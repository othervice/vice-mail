#!/usr/bin/env python3
"""Talks to the local test server for the desktop test (account-sync.e2e.mjs). Standard library only.

    python3 testmail.py status           alice's folders as JSON: {"INBOX": {"messages": n, "unseen": n}, ...}
    python3 testmail.py send SUBJECT     one message from bob to alice (SMTP submission, STARTTLS)

Host, ports, accounts and the CA come from testmail/accounts.env, as for every other client of
the test server. Nothing here changes alice's mailbox except the delivery itself.
"""

import base64
import imaplib
import json
import re
import smtplib
import ssl
import sys
import time
from email.message import EmailMessage
from email.utils import formatdate, make_msgid
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
ACCOUNTS_ENV = REPO / "testmail" / "accounts.env"


def load_env() -> dict[str, str]:
    env = {}
    for line in ACCOUNTS_ENV.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        line = line.removeprefix("export ")
        key, _, value = line.partition("=")
        env[key.strip()] = value.strip().strip('"')
    return env


def tls_context(env: dict[str, str]) -> ssl.SSLContext:
    return ssl.create_default_context(cafile=str(REPO / env["TESTMAIL_CA_FILE"]))


def mutf7_decode(name: str) -> str:
    """Modified UTF-7 of IMAP folder names (RFC 3501, 5.1.3)."""
    out, i = [], 0
    while i < len(name):
        if name[i] != "&":
            out.append(name[i])
            i += 1
            continue
        end = name.index("-", i)
        chunk = name[i + 1 : end]
        if chunk:
            raw = chunk.replace(",", "/")
            raw += "=" * (-len(raw) % 4)
            out.append(base64.b64decode(raw).decode("utf-16-be"))
        else:
            out.append("&")
        i = end + 1
    return "".join(out)


LIST_LINE = re.compile(r'^\((?P<flags>[^)]*)\) (?P<delim>"[^"]*"|NIL) (?P<name>.+)$')


def unquote(value: str) -> str:
    if value.startswith('"') and value.endswith('"'):
        return value[1:-1].replace('\\"', '"').replace("\\\\", "\\")
    return value


def status() -> dict:
    env = load_env()
    folders = {}
    with imaplib.IMAP4_SSL(
        env["TESTMAIL_HOST"], int(env["TESTMAIL_IMAPS_PORT"]), ssl_context=tls_context(env), timeout=30
    ) as imap:
        imap.login(env["TESTMAIL_ALICE_ADDRESS"], env["TESTMAIL_ALICE_PASSWORD"])
        typ, lines = imap.list()
        if typ != "OK":
            raise RuntimeError(f"LIST failed: {lines}")
        for raw in lines:
            match = LIST_LINE.match(raw.decode("ascii"))
            if not match:
                raise RuntimeError(f"unexpected LIST line: {raw!r}")
            flags = match["flags"].lower().split()
            wire = unquote(match["name"])
            if "\\noselect" in flags or "\\nonexistent" in flags:
                continue
            typ, data = imap.status(f'"{wire}"', "(MESSAGES UNSEEN)")
            if typ != "OK":
                raise RuntimeError(f"STATUS {wire} failed: {data}")
            counts = dict(re.findall(r"(MESSAGES|UNSEEN) (\d+)", data[0].decode("ascii")))
            folders[mutf7_decode(wire)] = {
                "messages": int(counts["MESSAGES"]),
                "unseen": int(counts["UNSEEN"]),
                "flags": [f for f in flags if f not in ("\\haschildren", "\\hasnochildren")],
            }
        imap.logout()
    return folders


def send(subject: str) -> dict:
    env = load_env()
    message = EmailMessage()
    message["From"] = f"Bob Example <{env['TESTMAIL_BOB_ADDRESS']}>"
    message["To"] = env["TESTMAIL_ALICE_ADDRESS"]
    message["Subject"] = subject
    message["Date"] = formatdate(localtime=True)
    message["Message-ID"] = make_msgid(domain=env["TESTMAIL_DOMAIN"])
    message.set_content("Delivered by the desktop test while the app is running.\n")
    started = time.time()
    # An explicit EHLO name: smtplib would otherwise look up the machine's FQDN first, which can
    # take seconds and would count as delivery time.
    with smtplib.SMTP(
        env["TESTMAIL_HOST"], int(env["TESTMAIL_SUBMISSION_PORT"]), local_hostname="localhost", timeout=30
    ) as smtp:
        smtp.starttls(context=tls_context(env))
        smtp.login(env["TESTMAIL_BOB_ADDRESS"], env["TESTMAIL_BOB_PASSWORD"])
        smtp.send_message(message)
    return {
        "messageId": message["Message-ID"],
        "startedAt": round(started * 1000),
        "acceptedAt": round(time.time() * 1000),
    }


def main() -> int:
    if len(sys.argv) >= 2 and sys.argv[1] == "status":
        print(json.dumps(status()))
        return 0
    if len(sys.argv) == 3 and sys.argv[1] == "send":
        print(json.dumps(send(sys.argv[2])))
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
