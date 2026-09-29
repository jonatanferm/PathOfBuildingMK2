#!/usr/bin/env python3
"""Fetch top poe.ninja characters' PoB exports into .parity/builds/*.xml."""
import base64, json, os, sys, time, urllib.request, zlib, re
sys.path.insert(0, os.path.dirname(__file__))
from pb import parse

OUT = os.path.join(os.path.dirname(__file__), "..", "..", ".parity", "builds")
UA = {"User-Agent": "PathOfBuildingMK2-parity/0.1"}

def get(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=30) as r:
        return r.read()

def name_account_lists(b, acc):
    try: fs = parse(b)
    except Exception: return
    for f, t, v in fs:
        if t == 2 and len(v) > 2:
            try: sub = parse(v)
            except Exception: continue
            ids = [x for x in sub if x[1] == 2]
            if ids and ids[0][2] in (b"name", b"account"):
                acc[ids[0][2].decode()] = [x[2].decode("utf-8", "replace") for x in ids[1:]]
            name_account_lists(v, acc)

def main():
    league = sys.argv[1] if len(sys.argv) > 1 else "allflame"
    kind = sys.argv[2] if len(sys.argv) > 2 else "exp"
    limit = int(sys.argv[3]) if len(sys.argv) > 3 else 100
    idx = json.loads(get("https://poe.ninja/poe1/api/data/index-state"))
    snap = next(s for s in idx["snapshotVersions"] if s["url"] == league and s["type"] == kind)
    ver = snap["version"]
    acc = {}
    name_account_lists(get(f"https://poe.ninja/poe1/api/builds/{ver}/search?overview={league}&type={kind}"), acc)
    pairs = list(zip(acc["name"], acc["account"]))[1:limit + 1]
    os.makedirs(OUT, exist_ok=True)
    n = 0
    for name, account in pairs:
        fn = os.path.join(OUT, f"{league}_{kind}_{n:03d}.xml")
        n += 1
        if os.path.exists(fn):
            continue
        q = urllib.parse.urlencode({"account": account, "name": name, "overview": league, "type": kind})
        try:
            ch = json.loads(get(f"https://poe.ninja/poe1/api/builds/{ver}/character?{q}"))
            code = ch["pathOfBuildingExport"]
            xml = zlib.decompress(base64.urlsafe_b64decode(code + "=" * (-len(code) % 4))).decode("utf-8")
        except Exception as e:
            print("skip", name, e, file=sys.stderr)
            continue
        with open(fn, "w") as f:
            f.write(xml)
        print(fn, ch.get("class"), ch.get("level"))
        time.sleep(0.4)

if __name__ == "__main__":
    import urllib.parse
    main()
