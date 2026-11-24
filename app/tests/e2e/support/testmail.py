#!/usr/bin/env python3
"""Talks to the local test server for the desktop tests. Standard library only.

    python3 testmail.py status                 alice's folders as JSON: {"INBOX": {"messages": n, "unseen": n}, ...}
    python3 testmail.py send SUBJECT           one message from bob to alice (SMTP submission, STARTTLS)
    python3 testmail.py append [--seen] FOLDER FILE...
                                               creates FOLDER in alice's account and appends the .eml files
                                               unchanged (reading.e2e.mjs: the hostile corpus); with
                                               --seen they are stored as read
    python3 testmail.py copy-seed PREFIX FOLDER
                                               copies every seed message whose X-Vicemail-Seed starts with
                                               PREFIX (for example `broken/`) into a new FOLDER, server side
    python3 testmail.py attachments FILE       SHA-256 and size of every attachment of an .eml file (no server)

Host, ports, accounts and the CA come from testmail/accounts.env, as for every other client of
the test server. Nothing here changes alice's mailbox except the delivery, the appended
messages and the copies; the seeded messages themselves stay as they are.
"""

import base64
import email
import hashlib
import email.utils
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


def login(env: dict[str, str]) -> imaplib.IMAP4_SSL:
    imap = imaplib.IMAP4_SSL(
        env["TESTMAIL_HOST"], int(env["TESTMAIL_IMAPS_PORT"]), ssl_context=tls_context(env), timeout=60
    )
    imap.login(env["TESTMAIL_ALICE_ADDRESS"], env["TESTMAIL_ALICE_PASSWORD"])
    return imap


def check(typ: str, data, what: str):
    if typ != "OK":
        raise RuntimeError(f"{what} failed: {data}")
    return data


def quoted(folder: str) -> str:
    """Folder argument for imaplib (ASCII names only; the test folders are)."""
    if not folder.isascii() or '"' in folder or "\\" in folder:
        raise ValueError(f"unsupported folder name: {folder!r}")
    return f'"{folder}"'


def internal_date(raw: bytes) -> float:
    """The Date header as INTERNALDATE, so the list sorts the message as its sender dated it."""
    header = email.message_from_bytes(raw.split(b"\r\n\r\n", 1)[0] + b"\r\n\r\n")
    try:
        return email.utils.parsedate_to_datetime(header["Date"]).timestamp()
    except (TypeError, ValueError):
        return time.time()


def append(folder: str, files: list[str], seen: bool = False) -> dict:
    env = load_env()
    imap = login(env)
    try:
        # A new folder (reading.e2e.mjs) or one that exists, such as INBOX (find-in-message.e2e.mjs).
        typ, data = imap.create(quoted(folder))
        if typ != "OK" and not any(isinstance(d, bytes) and b"ALREADYEXISTS" in d for d in data):
            check(typ, data, f"CREATE {folder}")
        appended = []
        for name in files:
            raw = Path(name).read_bytes()
            flags = "(\\Seen)" if seen else None
            check(*imap.append(quoted(folder), flags, internal_date(raw), raw), f"APPEND {name}")
            appended.append(Path(name).name)
        check(*imap.select(quoted(folder), readonly=True), f"EXAMINE {folder}")
        count = len(check(*imap.search(None, "ALL"), "SEARCH")[0].split())
        imap.logout()
    except BaseException:
        imap.shutdown()
        raise
    return {"folder": folder, "appended": appended, "messages": count}


SEED_HEADER = re.compile(rb"^X-Vicemail-Seed:[ \t]*(\S+)", re.IGNORECASE | re.MULTILINE)
FETCH_UID = re.compile(rb"UID (\d+)")


def copy_seed(prefix: str, target: str) -> dict:
    env = load_env()
    imap = login(env)
    copied = []
    try:
        folders = [f for f in selectable_folders(imap) if f != target]
        check(*imap.create(quoted(target)), f"CREATE {target}")
        for folder in folders:
            check(*imap.select(quoted(folder), readonly=True), f"EXAMINE {folder}")
            typ, data = imap.uid("FETCH", "1:*", "(UID BODY.PEEK[HEADER.FIELDS (X-VICEMAIL-SEED)])")
            check(typ, data, f"FETCH {folder}")
            uids = []
            for item in data:
                if not isinstance(item, tuple):
                    continue
                uid = FETCH_UID.search(item[0])
                key = SEED_HEADER.search(item[1])
                if uid and key and key.group(1).decode("ascii").startswith(prefix):
                    uids.append(uid.group(1).decode("ascii"))
                    copied.append({"key": key.group(1).decode("ascii"), "folder": folder})
            if uids:
                check(*imap.uid("COPY", ",".join(uids), quoted(target)), f"COPY from {folder}")
        imap.logout()
    except BaseException:
        imap.shutdown()
        raise
    return {"folder": target, "copied": sorted(copied, key=lambda c: c["key"])}


def selectable_folders(imap: imaplib.IMAP4_SSL) -> list[str]:
    """Selectable folder names (raw wire names; the seed's non-ASCII folder stays encoded)."""
    names = []
    for raw in check(*imap.list(), "LIST"):
        match = LIST_LINE.match(raw.decode("ascii"))
        if not match:
            raise RuntimeError(f"unexpected LIST line: {raw!r}")
        flags = match["flags"].lower().split()
        if "\\noselect" in flags or "\\nonexistent" in flags:
            continue
        names.append(unquote(match["name"]))
    return names


def attachments(path: str) -> list:
    """The decoded parts a mail program would offer as files, independently of the app's parser."""
    message = email.message_from_bytes(Path(path).read_bytes())
    found = []
    for part in message.walk():
        if part.is_multipart():
            continue
        if part.get_content_disposition() == "attachment" or part.get_filename() is not None:
            data = part.get_payload(decode=True) or b""
            found.append({"sha256": hashlib.sha256(data).hexdigest(), "size": len(data)})
    return found


def main() -> int:
    if len(sys.argv) >= 2 and sys.argv[1] == "status":
        print(json.dumps(status()))
        return 0
    if len(sys.argv) == 3 and sys.argv[1] == "send":
        print(json.dumps(send(sys.argv[2])))
        return 0
    if len(sys.argv) >= 5 and sys.argv[1:3] == ["append", "--seen"]:
        print(json.dumps(append(sys.argv[3], sys.argv[4:], seen=True)))
        return 0
    if len(sys.argv) >= 4 and sys.argv[1] == "append":
        print(json.dumps(append(sys.argv[2], sys.argv[3:])))
        return 0
    if len(sys.argv) == 3 and sys.argv[1] == "attachments":
        print(json.dumps(attachments(sys.argv[2])))
        return 0
    if len(sys.argv) == 4 and sys.argv[1] == "copy-seed":
        print(json.dumps(copy_seed(sys.argv[2], sys.argv[3])))
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
