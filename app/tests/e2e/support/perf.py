#!/usr/bin/env python3
"""Helpers of the performance measurement (tests/e2e/perf.e2e.mjs) for what Node cannot do.

  perf.py evict FILE_OR_DIR...   rewrite every file uncached (F_NOCACHE), so its pages leave
                                 the file cache without `sudo purge`; prints the bytes rewritten
  perf.py copy SRC DST           copy a file uncached (a fresh, uncached app binary); keeps the mode
  perf.py resident FILE_OR_DIR... how much of the files is in the file cache (mmap + mincore)
  perf.py pages FILE             which pages of the file are in the file cache
  perf.py memory PID             the app and its WebKit processes (WebContent, Networking, GPU),
                                 with RSS and physical footprint

Every command prints one JSON object. macOS only; standard library only.
"""
import ctypes
import ctypes.util
import fcntl
import json
import os
import subprocess
import sys

F_NOCACHE = getattr(fcntl, "F_NOCACHE", 48)
CHUNK = 4 * 1024 * 1024
libc = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)
libc.mmap.restype = ctypes.c_void_p
libc.mmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_longlong]
libc.munmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
libc.mincore.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_char_p]
PAGE = os.sysconf("SC_PAGE_SIZE")
PROT_READ, MAP_SHARED = 0x1, 0x1
MAP_FAILED = ctypes.c_void_p(-1).value


def files_under(paths):
    for path in paths:
        if os.path.isdir(path):
            for root, _dirs, names in os.walk(path):
                for name in names:
                    yield os.path.join(root, name)
        elif os.path.isfile(path):
            yield path


def copy_uncached(src, dst):
    """Reads and writes with F_NOCACHE: neither file keeps pages in the file cache."""
    size = 0
    with open(src, "rb", buffering=0) as fin, open(dst, "wb", buffering=0) as fout:
        fcntl.fcntl(fin.fileno(), F_NOCACHE, 1)
        fcntl.fcntl(fout.fileno(), F_NOCACHE, 1)
        while True:
            chunk = fin.read(CHUNK)
            if not chunk:
                break
            fout.write(chunk)
            size += len(chunk)
        os.fsync(fout.fileno())
    os.chmod(dst, os.stat(src).st_mode & 0o7777)
    return size


def evict(paths):
    total = count = 0
    for path in list(files_under(paths)):
        if os.path.islink(path):
            continue
        tmp = f"{path}.perf-evict"
        total += copy_uncached(path, tmp)
        # The old inode, with whatever of it was cached, goes away with the rename.
        os.replace(tmp, path)
        count += 1
    return {"files": count, "bytes": total}


def resident(paths):
    total_pages = in_core = 0
    for path in files_under(paths):
        size = os.path.getsize(path)
        if size == 0:
            continue
        pages = (size + PAGE - 1) // PAGE
        fd = os.open(path, os.O_RDONLY)
        try:
            addr = libc.mmap(None, size, PROT_READ, MAP_SHARED, fd, 0)
            if addr in (None, MAP_FAILED):
                raise OSError(ctypes.get_errno(), f"mmap {path}")
            try:
                vec = ctypes.create_string_buffer(pages)
                if libc.mincore(addr, size, vec) != 0:
                    raise OSError(ctypes.get_errno(), f"mincore {path}")
                in_core += sum(1 for b in vec.raw[:pages] if b & 1)
            finally:
                libc.munmap(addr, size)
        finally:
            os.close(fd)
        total_pages += pages
    return {"pages": total_pages, "resident": in_core, "fraction": (in_core / total_pages) if total_pages else 0.0}


def resident_pages(path):
    """Indices of the file's pages (of PAGE bytes) that are in the file cache."""
    size = os.path.getsize(path)
    pages = (size + PAGE - 1) // PAGE
    fd = os.open(path, os.O_RDONLY)
    try:
        addr = libc.mmap(None, size, PROT_READ, MAP_SHARED, fd, 0)
        if addr in (None, MAP_FAILED):
            raise OSError(ctypes.get_errno(), f"mmap {path}")
        try:
            vec = ctypes.create_string_buffer(pages)
            if libc.mincore(addr, size, vec) != 0:
                raise OSError(ctypes.get_errno(), f"mincore {path}")
            return {"pageSize": PAGE, "pages": pages, "resident": [i for i, b in enumerate(vec.raw[:pages]) if b & 1]}
        finally:
            libc.munmap(addr, size)
    finally:
        os.close(fd)


class RusageInfoV0(ctypes.Structure):
    _fields_ = [
        ("uuid", ctypes.c_uint8 * 16),
        ("user_time", ctypes.c_uint64),
        ("system_time", ctypes.c_uint64),
        ("pkg_idle_wkups", ctypes.c_uint64),
        ("interrupt_wkups", ctypes.c_uint64),
        ("pageins", ctypes.c_uint64),
        ("wired_size", ctypes.c_uint64),
        ("resident_size", ctypes.c_uint64),
        ("phys_footprint", ctypes.c_uint64),
        ("proc_start_abstime", ctypes.c_uint64),
        ("proc_exit_abstime", ctypes.c_uint64),
    ]


def memory(app_pid):
    """The app and its WebKit processes. WebKit's XPC services count as the app's when the
    system holds the app responsible for them, or the process responsible for the app (an app
    started from a terminal or a launchd job, as in the test, shares that process's
    responsibility; one started from the Finder is responsible for itself)."""
    proc = ctypes.CDLL(ctypes.util.find_library("proc"))
    responsible = libc.responsibility_get_pid_responsible_for_pid
    responsible.restype = ctypes.c_int
    responsible.argtypes = [ctypes.c_int]
    owners = {app_pid, responsible(app_pid)}
    listing = subprocess.run(["ps", "-axo", "pid=,rss=,comm="], capture_output=True, text=True, check=True).stdout
    rows = []
    others = []
    for line in listing.splitlines():
        parts = line.strip().split(None, 2)
        if len(parts) < 3:
            continue
        pid, rss_kb, command = int(parts[0]), int(parts[1]), parts[2]
        webkit = "com.apple.WebKit" in command
        owner = responsible(pid)
        if pid != app_pid and not (webkit and owner in owners):
            if webkit:
                others.append({"pid": pid, "name": os.path.basename(command), "responsible": owner, "rssBytes": rss_kb * 1024})
            continue
        info = RusageInfoV0()
        footprint = None
        if proc.proc_pid_rusage(pid, 0, ctypes.byref(info)) == 0:
            footprint = info.phys_footprint
        rows.append(
            {
                "pid": pid,
                "name": os.path.basename(command),
                "rssBytes": rss_kb * 1024,
                "footprintBytes": footprint,
            }
        )
    return {
        "processes": rows,
        "otherWebKit": others,
        "rssBytes": sum(r["rssBytes"] for r in rows),
        "footprintBytes": sum(r["footprintBytes"] or 0 for r in rows),
        "footprintMissing": [r["name"] for r in rows if r["footprintBytes"] is None],
    }


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    command, args = argv[1], argv[2:]
    if command == "evict":
        result = evict(args)
    elif command == "copy":
        result = {"bytes": copy_uncached(args[0], args[1])}
    elif command == "resident":
        result = resident(args)
    elif command == "pages":
        result = resident_pages(args[0])
    elif command == "memory":
        result = memory(int(args[0]))
    else:
        print(f"unknown command {command}", file=sys.stderr)
        return 2
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
