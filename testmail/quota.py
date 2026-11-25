#!/usr/bin/env python3
"""Sets or clears the storage quota of a test account (stage 20, decision 0060). Standard library
only; talks to the management API on 127.0.0.1 as the administrator of the test server.

    python3 testmail/quota.py set ACCOUNT BYTES     e.g. `set alice 12000000`
    python3 testmail/quota.py fill ACCOUNT PERCENT  a limit so that the current use is PERCENT %
    python3 testmail/quota.py clear ACCOUNT         no quota (as provisioned)

Prints the account's quotas and the bytes in use as JSON. Only for the local test server.
"""

import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True  # keep testmail/ free of __pycache__
sys.path.insert(0, str(Path(__file__).resolve().parent))
import provision  # noqa: E402  (the management API client of the provisioning)


def main() -> int:
    if len(sys.argv) < 3 or sys.argv[1] not in ("set", "fill", "clear"):
        print(__doc__, file=sys.stderr)
        return 2
    command, name = sys.argv[1], sys.argv[2]
    env = provision.load_env()
    url = f"http://{env['TESTMAIL_HOST']}:{env['TESTMAIL_HTTP_PORT']}"
    api = provision.Jmap(url, env["TESTMAIL_ADMIN_ADDRESS"], env["TESTMAIL_ADMIN_PASSWORD"])
    accounts = [a for a in api.get("Account", None) if a.get("name") == name]
    if not accounts:
        print(f"no account {name}", file=sys.stderr)
        return 1
    account = accounts[0]
    if command == "clear":
        quotas = {}
    elif command == "set":
        quotas = {"maxDiskQuota": int(sys.argv[3])}
    else:
        percent = int(sys.argv[3])
        used = int(account.get("usedDiskQuota") or 0)
        quotas = {"maxDiskQuota": max(1, used * 100 // max(1, percent))}
    api.call(["x:Account/set", {"update": {account["id"]: {"quotas": quotas}}}, "q"])
    [after] = api.get("Account", [account["id"]])
    print(json.dumps({"quotas": after.get("quotas"), "used": after.get("usedDiskQuota")}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
