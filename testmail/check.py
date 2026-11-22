#!/usr/bin/env python3
"""Prove that the local test server offers what Vice Mail relies on.

    check.py

Checks TLS (implicit and STARTTLS), CAPABILITY, SPECIAL-USE folders, custom
keywords (PERMANENTFLAGS with \\*, STORE and read back), CONDSTORE
(HIGHESTMODSEQ, CHANGEDSINCE, UNCHANGEDSINCE), MOVE, IDLE woken up by a real
SMTP delivery, and the SMTP rules (AUTH required on submission, no relaying).

Leaves no messages behind: the scratch mailboxes are deleted and the IDLE test
message is expunged again (UIDNEXT and MODSEQ of bob's INBOX do advance).
"""

from __future__ import annotations

import argparse
import re
import secrets
import smtplib
import socket
import ssl
import sys
import time
from email.utils import format_datetime
from datetime import datetime, timezone
from pathlib import Path

sys.dont_write_bytecode = True  # keep testmail/ free of __pycache__
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Imap, ImapError, load_env, quote, tls_context  # noqa: E402

REQUIRED_CAPS = ["IMAP4rev1", "IDLE", "CONDSTORE", "MOVE", "SPECIAL-USE", "UIDPLUS", "LITERAL+", "ENABLE"]
SPECIAL_USE = {"\\Sent": "Sent", "\\Drafts": "Drafts", "\\Trash": "Trash", "\\Junk": "Junk",
               "\\Archive": "Archive"}


class Checker:
    def __init__(self, env: dict[str, str]) -> None:
        self.env = env
        self.ctx = tls_context(env)
        self.host = env["TESTMAIL_HOST"]
        self.failures: list[str] = []
        self.passes = 0
        self.capability_line = ""

    def ok(self, name: str, detail: str = "") -> None:
        self.passes += 1
        print(f"PASS  {name}" + (f": {detail}" if detail else ""))

    def fail(self, name: str, detail: str) -> None:
        self.failures.append(name)
        print(f"FAIL  {name}: {detail}")

    def expect(self, cond: bool, name: str, detail: str = "", failure: str = "") -> bool:
        if cond:
            self.ok(name, detail)
        else:
            self.fail(name, failure or detail or "condition not met")
        return cond

    def imaps(self, user: str | None = None, password: str | None = None,
              server_hostname: str | None = None) -> Imap:
        imap = Imap(self.host, int(self.env["TESTMAIL_IMAPS_PORT"]), self.ctx,
                    server_hostname=server_hostname or self.env["TESTMAIL_TLS_NAME"])
        if user:
            imap.login(user, password or "")
        return imap

    def alice(self) -> Imap:
        return self.imaps(self.env["TESTMAIL_ALICE_ADDRESS"], self.env["TESTMAIL_ALICE_PASSWORD"])

    def bob(self) -> Imap:
        return self.imaps(self.env["TESTMAIL_BOB_ADDRESS"], self.env["TESTMAIL_BOB_PASSWORD"])

    # -- checks ------------------------------------------------------------

    def check_tls(self) -> None:
        for name in (self.env["TESTMAIL_TLS_NAME"], self.host):
            imap = self.imaps(server_hostname=name)
            self.ok(f"IMAPS {self.host}:{self.env['TESTMAIL_IMAPS_PORT']} verified against the test CA",
                    f"name {name}, {imap.sock.version()}")
            imap.logout()
        try:
            sock = socket.create_connection((self.host, int(self.env["TESTMAIL_IMAPS_PORT"])), timeout=10)
            ssl.create_default_context().wrap_socket(sock, server_hostname="localhost").close()
            self.fail("certificate is not trusted without the test CA", "handshake succeeded")
        except ssl.SSLCertVerificationError:
            self.ok("certificate is not trusted without the test CA")

        imap = Imap(self.host, int(self.env["TESTMAIL_IMAP_PORT"]), None, implicit_tls=False)
        caps = imap.capabilities()
        self.expect("STARTTLS" in caps and "LOGINDISABLED" in caps,
                    f"IMAP {self.env['TESTMAIL_IMAP_PORT']} offers STARTTLS and refuses LOGIN before it",
                    failure=" ".join(caps))
        tagged, _ = imap.command(f"LOGIN {quote(self.env['TESTMAIL_ALICE_ADDRESS'])} "
                                 f"{quote(self.env['TESTMAIL_ALICE_PASSWORD'])}", check=False)
        self.expect(b" OK" not in tagged, "LOGIN without TLS is rejected", tagged.decode().strip())
        imap.starttls(self.ctx, self.env["TESTMAIL_TLS_NAME"])
        imap.login(self.env["TESTMAIL_ALICE_ADDRESS"], self.env["TESTMAIL_ALICE_PASSWORD"])
        self.ok("STARTTLS and LOGIN on port " + self.env["TESTMAIL_IMAP_PORT"], imap.sock.version())
        imap.logout()

    def check_failed_logins(self) -> None:
        """Wrong passwords fail fast, and repeated failures do not ban 127.0.0.1."""
        started = time.monotonic()
        for _ in range(5):
            imap = self.imaps()
            tagged, _ = imap.command(f"LOGIN {quote(self.env['TESTMAIL_ALICE_ADDRESS'])} \"wrong\"", check=False)
            imap.close()
            if b" OK" in tagged:
                self.fail("wrong IMAP password is rejected", tagged.decode().strip())
                return
        try:
            smtp = smtplib.SMTP_SSL(self.host, int(self.env["TESTMAIL_SMTPS_PORT"]), context=self.ctx, timeout=30,
                                    local_hostname=f"client.{self.env['TESTMAIL_DOMAIN']}")
            smtp.login(self.env["TESTMAIL_ALICE_ADDRESS"], "wrong")
            self.fail("wrong SMTP password is rejected", "login succeeded")
        except smtplib.SMTPAuthenticationError as err:
            smtp.close()
            self.ok("wrong SMTP password is rejected", f"{err.smtp_code}")
        elapsed = time.monotonic() - started
        self.alice().logout()
        self.ok("5 wrong IMAP logins are rejected, no auto-ban afterwards", f"{elapsed * 1000:.0f} ms in total")

    def check_capabilities(self) -> None:
        imap = self.alice()
        caps = imap.capabilities()
        self.capability_line = " ".join(caps)
        print(f"      CAPABILITY {self.capability_line}")
        upper = {c.upper() for c in caps}
        missing = [c for c in REQUIRED_CAPS if c.upper() not in upper]
        self.expect(not missing, "CAPABILITY lists " + ", ".join(REQUIRED_CAPS), failure=f"missing {missing}")
        imap.logout()

    def check_special_use(self) -> None:
        imap = self.alice()
        _, untagged = imap.command('LIST "" "*" RETURN (SPECIAL-USE)')
        found: dict[str, str] = {}
        for line in untagged:
            match = re.match(rb'\* LIST \(([^)]*)\) "[^"]*" (.+)\r\n$', line)
            if not match:
                continue
            name = match.group(2).decode().strip('"')
            for attr in match.group(1).decode().split():
                if attr in SPECIAL_USE:
                    found[attr] = name
        self.expect(found == SPECIAL_USE, "SPECIAL-USE attributes on Sent, Drafts, Trash, Junk, Archive",
                    ", ".join(f"{k} {v}" for k, v in sorted(found.items())), failure=f"got {found}")
        _, untagged = imap.command('LIST (SPECIAL-USE) "" "*"')
        self.expect(len(untagged) == len(SPECIAL_USE), "LIST (SPECIAL-USE) selection returns only those",
                    f"{len(untagged)} mailboxes", failure=b" | ".join(untagged).decode())
        imap.logout()

    def check_keywords_condstore_move(self) -> None:
        suffix = secrets.token_hex(3)
        source, target = f"VMCheck-{suffix}", f"VMCheck-{suffix}-target"
        imap = self.alice()
        try:
            imap.command(f"CREATE {quote(source)}")
            imap.command(f"CREATE {quote(target)}")
            uids = []
            for i in range(2):
                raw = (f"From: check@example.org\r\nTo: {self.env['TESTMAIL_ALICE_ADDRESS']}\r\n"
                       f"Subject: check {i}\r\nMessage-ID: <check-{suffix}-{i}@vicemail.test>\r\n"
                       f"X-Vicemail-Seed: check/{suffix}-{i}\r\n\r\nbody {i}\r\n").encode()
                tag = imap.next_tag()
                imap.send(tag + f" APPEND {quote(source)} () {{{len(raw)}+}}\r\n".encode() + raw + b"\r\n")
                tagged, _ = imap.wait(tag)
                match = re.search(rb"APPENDUID (\d+) (\d+)", tagged)
                uids.append(int(match.group(2)) if match else None)
            self.expect(all(uids), "APPEND returns APPENDUID (UIDPLUS)", f"UIDs {uids}")

            _, untagged = imap.command(f"SELECT {quote(source)} (CONDSTORE)")
            text = b"".join(untagged)
            perm = re.search(rb"PERMANENTFLAGS \(([^)]*)\)", text)
            self.expect(bool(perm) and b"\\*" in perm.group(1), "PERMANENTFLAGS contains \\* (custom keywords)",
                        perm.group(0).decode() if perm else "", failure=text.decode(errors="replace"))
            hms = re.search(rb"HIGHESTMODSEQ (\d+)", text)
            if not self.expect(bool(hms), "SELECT (CONDSTORE) reports HIGHESTMODSEQ",
                               hms.group(0).decode() if hms else "", failure=text.decode(errors="replace")):
                return
            modseq0 = int(hms.group(1))

            keywords = ["$test", "$label1", "Custom_Keyword-123"]
            _, untagged = imap.command(f"UID STORE {uids[0]} +FLAGS ({' '.join(keywords)})")
            store_line = b"".join(untagged)
            self.expect(b"MODSEQ" in store_line, "STORE answers with MODSEQ", store_line.decode().strip())

            # Read back on a fresh connection, so it is really stored.
            other = self.alice()
            other.command(f"EXAMINE {quote(source)}")
            _, untagged = other.command(f"UID FETCH {uids[0]} (FLAGS)")
            other.logout()
            flags = b"".join(untagged).decode()
            # Keywords are case-insensitive (RFC 3501, section 9); Stalwart stores them lowercased.
            self.expect(all(k.lower() in flags.lower() for k in keywords),
                        "custom keywords persist (read back on a new connection)", flags.strip())
            if not all(k in flags for k in keywords):
                print("      note: the server returns keywords lowercased (Custom_Keyword-123 -> custom_keyword-123)")

            _, untagged = imap.command(f"UID FETCH 1:* (FLAGS) (CHANGEDSINCE {modseq0})")
            changed = [int(m) for m in re.findall(rb"UID (\d+)", b"".join(untagged))]
            self.expect(changed == [uids[0]], f"FETCH (CHANGEDSINCE {modseq0}) returns only the changed message",
                        f"UIDs {changed}")

            tagged, _ = imap.command(f"UID STORE {uids[1]} (UNCHANGEDSINCE 1) +FLAGS (\\Flagged)", check=False)
            self.expect(b"MODIFIED" in tagged, "STORE (UNCHANGEDSINCE) is refused with MODIFIED",
                        tagged.decode().strip())

            tagged, untagged = imap.command(f"UID MOVE {uids[0]} {quote(target)}")
            text = b"".join(untagged) + tagged
            self.expect(b"COPYUID" in text, "UID MOVE answers with COPYUID",
                        (re.search(rb"COPYUID [^\]]*", text) or re.search(rb".*", text)).group(0).decode())
            _, untagged = imap.command(f"STATUS {quote(target)} (MESSAGES)")
            moved = re.search(rb"MESSAGES (\d+)", b"".join(untagged))
            _, untagged = imap.command("UID SEARCH ALL")
            left = re.findall(rb"\d+", b"".join(untagged).split(b"SEARCH", 1)[-1])
            self.expect(moved and moved.group(1) == b"1" and left == [str(uids[1]).encode()],
                        "MOVE removed the message from the source and stored it in the target")
            imap.command(f"EXAMINE {quote(target)}")
            _, untagged = imap.command("FETCH 1 (FLAGS)")
            flags = b"".join(untagged).decode()
            self.expect("$test" in flags, "keywords survive MOVE", flags.strip())
        finally:
            imap.command("UNSELECT", check=False)
            for name in (target, source):
                imap.command(f"DELETE {quote(name)}", check=False)
            imap.logout()

    def check_idle_with_smtp(self) -> None:
        bob = self.bob()
        try:
            _, untagged = bob.command("SELECT INBOX")
            exists = int(re.search(rb"\* (\d+) EXISTS", b"".join(untagged)).group(1))
            tag = bob.next_tag()
            bob.send(tag + b" IDLE\r\n")
            cont = bob.read_line()
            if not self.expect(cont.startswith(b"+"), "IDLE accepted", cont.decode().strip()):
                return
            msgid = f"<idle-check-{secrets.token_hex(4)}@vicemail.test>"
            raw = (f"Date: {format_datetime(datetime.now(timezone.utc))}\r\n"
                   f"From: {self.env['TESTMAIL_ALICE_ADDRESS']}\r\nTo: {self.env['TESTMAIL_BOB_ADDRESS']}\r\n"
                   f"Subject: IDLE check\r\nMessage-ID: {msgid}\r\nX-Vicemail-Seed: check/idle\r\n\r\n"
                   "Delivered by SMTP while bob is in IDLE.\r\n").encode()
            sent_at = time.monotonic()
            self.smtp_send(self.env["TESTMAIL_ALICE_ADDRESS"], self.env["TESTMAIL_ALICE_PASSWORD"],
                           [self.env["TESTMAIL_BOB_ADDRESS"]], raw)
            bob.sock.settimeout(15)
            woke = None
            while True:
                line = bob.read_line()
                match = re.match(rb"\* (\d+) EXISTS", line)
                if match and int(match.group(1)) > exists:
                    woke = time.monotonic() - sent_at
                    break
            bob.send(b"DONE\r\n")
            bob.wait(tag)
            self.ok("IDLE reports EXISTS for a mail delivered via SMTP submission", f"after {woke * 1000:.0f} ms")
            _, untagged = bob.command("UID SEARCH HEADER Message-ID " + quote(msgid))
            uids = re.findall(rb"\d+", b"".join(untagged).split(b"SEARCH", 1)[-1])
            if not uids:  # the search index may lag; fall back to the newest message
                _, untagged = bob.command("UID FETCH * (UID BODY.PEEK[HEADER.FIELDS (MESSAGE-ID)])")
                if msgid.encode() in b"".join(untagged):
                    uids = re.findall(rb"UID (\d+)", b"".join(untagged))
            self.expect(bool(uids), "the SMTP mail is in bob's INBOX", f"UID {b','.join(uids).decode()}")
            if uids:
                bob.command(f"UID STORE {uids[0].decode()} +FLAGS.SILENT (\\Deleted)")
                bob.command(f"UID EXPUNGE {uids[0].decode()}")
        except (TimeoutError, socket.timeout):
            self.fail("IDLE reports EXISTS for a mail delivered via SMTP submission", "no EXISTS within 15 s")
        finally:
            bob.close()

    def smtp_send(self, user: str, password: str, rcpts: list[str], raw: bytes) -> None:
        smtp = smtplib.SMTP(self.host, int(self.env["TESTMAIL_SUBMISSION_PORT"]),
                            local_hostname=f"client.{self.env['TESTMAIL_DOMAIN']}", timeout=30)
        smtp.starttls(context=self.ctx)
        smtp.login(user, password)
        smtp.sendmail(user, rcpts, raw)
        smtp.quit()

    def check_smtp(self) -> None:
        env, name = self.env, f"client.{self.env['TESTMAIL_DOMAIN']}"
        alice, alice_pw = env["TESTMAIL_ALICE_ADDRESS"], env["TESTMAIL_ALICE_PASSWORD"]
        port = int(env["TESTMAIL_SUBMISSION_PORT"])
        smtp = smtplib.SMTP(self.host, port, local_hostname=name, timeout=30)
        smtp.ehlo()
        plain_auth = smtp.has_extn("auth")
        self.expect(smtp.has_extn("starttls") and not plain_auth,
                    f"submission {port}: STARTTLS offered, AUTH only after TLS")
        smtp.starttls(context=self.ctx)
        smtp.ehlo()
        self.expect(smtp.has_extn("auth"), f"submission {port}: AUTH after STARTTLS",
                    smtp.esmtp_features.get("auth", ""))
        code, msg = smtp.mail(alice)
        if code == 250:
            code, msg = smtp.rcpt(env["TESTMAIL_BOB_ADDRESS"])
        self.expect(code >= 500, f"submission {port}: sending without AUTH is refused", f"{code} {msg.decode()}")
        smtp.close()

        smtp = smtplib.SMTP_SSL(self.host, int(env["TESTMAIL_SMTPS_PORT"]), local_hostname=name,
                                context=self.ctx, timeout=30)
        smtp.login(alice, alice_pw)
        self.ok(f"submissions {env['TESTMAIL_SMTPS_PORT']}: implicit TLS and AUTH")
        smtp.mail(alice)
        code, msg = smtp.rcpt("someone@example.com")
        self.expect(code >= 500, "authenticated relaying to external domains is refused (nothing leaves the machine)",
                    f"{code} {msg.decode()}")
        smtp.rset()
        code, msg = smtp.mail(env["TESTMAIL_BOB_ADDRESS"])
        if code == 250:
            code, msg = smtp.rcpt(alice)
        self.expect(code >= 500, "MAIL FROM another user's address is refused", f"{code} {msg.decode()}")
        smtp.close()

        port = int(env["TESTMAIL_SMTP_PORT"])
        smtp = smtplib.SMTP(self.host, port, local_hostname=name, timeout=30)
        smtp.ehlo()
        smtp.mail("sender@example.org")
        code, msg = smtp.rcpt(alice)
        self.expect(code == 250, f"local SMTP {port}: accepts mail for local users without AUTH", f"{code}")
        code, msg = smtp.rcpt("someone@example.com")
        self.expect(code >= 500, f"local SMTP {port}: refuses external recipients", f"{code} {msg.decode()}")
        smtp.rset()
        smtp.quit()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.parse_args()
    checker = Checker(load_env())
    started = time.monotonic()
    for step in (checker.check_tls, checker.check_failed_logins, checker.check_capabilities, checker.check_special_use,
                 checker.check_keywords_condstore_move, checker.check_idle_with_smtp, checker.check_smtp):
        try:
            step()
        except (ImapError, smtplib.SMTPException, OSError, AttributeError) as err:
            checker.fail(step.__name__, f"{type(err).__name__}: {err}")
    print(f"{checker.passes} passed, {len(checker.failures)} failed in {time.monotonic() - started:.1f} s")
    return 1 if checker.failures else 0


if __name__ == "__main__":
    sys.exit(main())
