"""Shared helpers for the local test server scripts (standard library only).

Used by seed.py, check.py and provision.py. Nothing here talks to anything
but 127.0.0.1.
"""

from __future__ import annotations

import os
import re
import socket
import ssl
from pathlib import Path

TESTMAIL_DIR = Path(__file__).resolve().parent
REPO_ROOT = TESTMAIL_DIR.parent
ACCOUNTS_ENV = TESTMAIL_DIR / "accounts.env"

_ENV_LINE = re.compile(r"^(?:export\s+)?([A-Z][A-Z0-9_]*)=(.*)$")


def load_env(path: Path = ACCOUNTS_ENV) -> dict[str, str]:
    """Parse accounts.env (KEY=VALUE lines, optional `export`, # comments)."""
    env: dict[str, str] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        match = _ENV_LINE.match(line)
        if not match:
            raise ValueError(f"{path}: cannot parse line: {raw!r}")
        key, value = match.groups()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in "'\"":
            value = value[1:-1]
        env[key] = value
    # Values from the real environment win, so tests can override single ports.
    for key in list(env):
        if key in os.environ:
            env[key] = os.environ[key]
    return env


def ca_file(env: dict[str, str]) -> Path:
    path = Path(env["TESTMAIL_CA_FILE"])
    if not path.is_absolute():
        path = REPO_ROOT / path
    if not path.is_file():
        raise SystemExit(
            f"CA file {path} is missing. Run 'scripts/testmail.sh start' first."
        )
    return path


def tls_context(env: dict[str, str]) -> ssl.SSLContext:
    """TLS context that trusts only the local test CA."""
    ctx = ssl.create_default_context(cafile=str(ca_file(env)))
    ctx.minimum_version = ssl.TLSVersion.TLSv1_2
    return ctx


class ImapError(Exception):
    pass


class Imap:
    """Minimal IMAP client with pipelining support.

    Responses are returned as lists of "lines"; a line that carried literals
    is returned with the literal bytes spliced in, so callers can search it.
    """

    def __init__(self, host: str, port: int, ctx: ssl.SSLContext | None,
                 *, implicit_tls: bool = True, server_hostname: str = "localhost",
                 timeout: float = 60.0) -> None:
        sock = socket.create_connection((host, port), timeout=timeout)
        if implicit_tls:
            assert ctx is not None
            sock = ctx.wrap_socket(sock, server_hostname=server_hostname)
        self.sock = sock
        self.file = sock.makefile("rb")
        self.tag_counter = 0
        self.greeting = self.read_line()
        if not self.greeting.startswith(b"* OK"):
            raise ImapError(f"unexpected greeting: {self.greeting!r}")

    # -- low level ---------------------------------------------------------

    def read_line(self) -> bytes:
        """Read one logical response line, including any literals."""
        out = bytearray()
        while True:
            line = self.file.readline()
            if not line:
                raise ImapError("connection closed by server")
            out += line
            match = re.search(rb"\{(\d+)\+?\}\r\n$", line)
            if not match:
                return bytes(out)
            size = int(match.group(1))
            data = self.file.read(size)
            if len(data) != size:
                raise ImapError("connection closed inside literal")
            out += data

    def next_tag(self) -> bytes:
        self.tag_counter += 1
        return b"T%05d" % self.tag_counter

    def send(self, data: bytes) -> None:
        self.sock.sendall(data)

    def command(self, cmd: str | bytes, *, check: bool = True) -> tuple[bytes, list[bytes]]:
        """Send a command (no synchronising literals) and wait for its tag."""
        if isinstance(cmd, str):
            cmd = cmd.encode("utf-8")
        tag = self.next_tag()
        self.send(tag + b" " + cmd + b"\r\n")
        return self.wait(tag, check=check, what=cmd)

    def wait(self, tag: bytes, *, check: bool = True, what: bytes = b"") -> tuple[bytes, list[bytes]]:
        untagged: list[bytes] = []
        while True:
            line = self.read_line()
            if line.startswith(tag + b" "):
                if check and not line[len(tag) + 1:].startswith(b"OK"):
                    raise ImapError(f"{what[:80]!r} failed: {line.strip()!r}")
                return line, untagged
            untagged.append(line)

    # -- helpers -----------------------------------------------------------

    def starttls(self, ctx: ssl.SSLContext, server_hostname: str = "localhost") -> None:
        self.command("STARTTLS")
        self.sock = ctx.wrap_socket(self.sock, server_hostname=server_hostname)
        self.file = self.sock.makefile("rb")

    def login(self, user: str, password: str) -> bytes:
        tagged, _ = self.command(f"LOGIN {quote(user)} {quote(password)}")
        return tagged

    def capabilities(self) -> list[str]:
        _, untagged = self.command("CAPABILITY")
        for line in untagged:
            if line.upper().startswith(b"* CAPABILITY "):
                return line[13:].decode().split()
        raise ImapError("no CAPABILITY response")

    def logout(self) -> None:
        try:
            self.command("LOGOUT", check=False)
        except (ImapError, OSError):
            pass
        self.close()

    def close(self) -> None:
        try:
            self.sock.close()
        except OSError:
            pass


def quote(value: str) -> str:
    """IMAP quoted string (callers only pass printable ASCII/UTF-8 text)."""
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def mutf7_encode(name: str) -> str:
    """Encode a mailbox name in modified UTF-7 (RFC 3501, section 5.1.3)."""
    import base64

    out: list[str] = []
    buf: list[str] = []

    def flush() -> None:
        if buf:
            data = "".join(buf).encode("utf-16-be")
            out.append("&" + base64.b64encode(data).decode().rstrip("=").replace("/", ",") + "-")
            buf.clear()

    for ch in name:
        if 0x20 <= ord(ch) <= 0x7E:
            flush()
            out.append("&-" if ch == "&" else ch)
        else:
            buf.append(ch)
    flush()
    return "".join(out)
