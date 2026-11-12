#!/usr/bin/env python3
"""Fetch the INT-1-FRESH parity corpus from poe.ninja (calc doc §11.1 row INT-1).

Usage:
    python3 -I scripts/port/fetch_fresh_corpus.py fetch [--out DIR] [--per-class N] [--alternates]
    python3 -I scripts/port/fetch_fresh_corpus.py manifest [--out DIR] [--probe DIR] > docs/port/parity/fresh-corpus.md

`fetch` reads poe.ninja's public build pages through the JSON/protobuf API the site itself uses:
  * /poe1/api/data/index-state                      league snapshots (version, passive tree);
  * /poe1/api/builds/<version>/search?overview=<snapshot>&type=exp&class=<ascendancy>
                                                    the first 100 characters of one class
                                                    (protobuf; value lists name/account/class/dps.skill);
  * /poe1/api/builds/dictionary/<hash>              the string tables the search result indexes (NDIC);
  * /poe1/api/builds/<version>/character?account=..&name=..&overview=<snapshot>&type=exp
                                                    one character's JSON, with `pathOfBuildingExport`.
It picks `--per-class` characters per ascendancy (spread over the leagues in LEAGUES), preferring
main skills and skill kinds (attack, spell, minion, totem/trap/mine/brand, DoT) not picked yet,
decodes each PoB code (url-safe base64 of a zlib stream) to XML and writes
`DIR/fresh_NNN.xml` plus `DIR/meta/fresh_NNN.json` (source URL, fetch date, league, class,
main skill, sha256). With `--alternates`, a pick whose character API answers 404 is replaced by
the next-best row of the same search (up to 3). Raw API responses (and 404s) are cached in
`DIR/raw/`, so a rerun sends no request it already made. The corpus of 2026-11-11/12 was made
with `fetch` and then `fetch --alternates`. DIR (default .parity/fresh) is gitignored: build XML is never committed.

Politeness: one request per second at most (`MIN_INTERVAL`), a generic User-Agent with no
personal data, robots.txt checked before the first request, 429/503 honour `Retry-After`.

`manifest` prints the committed manifest (docs/port/parity/fresh-corpus.md) from `DIR/meta`
and `DIR/datacheck.txt` (the summary of scripts/port/fresh_corpus_datacheck.lua's run),
the kept/dropped split (`DIR/*.xml` kept, `DIR/dropped/*.xml` dropped, reason in
`DIR/dropped/<stem>.reason`) and, when `--probe` names the CA test's PoB cache
(`target/tmp/ca-probe`), PoB's DoT/hit split for the kind column.
"""
import argparse
import base64
import datetime
import hashlib
import json
import os
import re
import struct
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import urllib.robotparser
import zlib

UA = "pob-port-parity-corpus/1.0"
BASE = "https://poe.ninja"
MIN_INTERVAL = 1.1  # seconds between requests (<= 1 request per second)

# (league url, share of picks); every league here is on a passive tree PoB 2.67.2 ships
# (3_29 / 3_28, src/GameVersions.lua:11-13). Allflame softcore trade is the all57 corpus's league,
# so it is left out.
LEAGUES = ["standard", "allflamehc", "allflamessf", "mirage", "standard"]
ASCENDANCIES = [
    "Ascendant", "Assassin", "Berserker", "Champion", "Chieftain", "Deadeye", "Elementalist",
    "Gladiator", "Guardian", "Hierophant", "Inquisitor", "Juggernaut", "Luminary", "Necromancer",
    "Occultist", "Pathfinder", "Reliquarian", "Saboteur", "Slayer", "Trickster", "Warden",
]
# skills whose damage is mainly damage over time (no gem tag says so)
DOT_SKILLS = {
    "Essence Drain", "Contagion", "Bane", "Blight", "Toxic Rain", "Caustic Arrow", "Righteous Fire",
    "Scorching Ray", "Vortex", "Cremation", "Soulrend", "Exsanguinate", "Corrupting Fever",
    "Death Aura", "Forbidden Rite", "Poisonous Concoction", "Wither", "Fire Trap", "Storm Burst",
    "Penance Brand of Dissipation", "Viper Strike", "Cobra Lash", "Pestilent Strike",
    "Bane of Condemnation", "Blight of Contagion", "Blight of Atrophy", "Toxic Rain of Withering",
    "Caustic Arrow of Poison", "Righteous Fire of Arcane Devotion", "Scorching Ray of Immolation",
    "Essence Drain of Desperation", "Exsanguinate of Transmission", "Reap", "Detonate Dead",
    "Volatile Dead", "Corpse Pact", "Sunder",
}


class Http:
    def __init__(self, raw_dir):
        self.raw_dir = raw_dir
        self.last = 0.0
        self.sent = 0
        self.robots = None

    def _cache_path(self, url):
        return os.path.join(self.raw_dir, hashlib.sha256(url.encode()).hexdigest()[:24] + ".bin")

    def get(self, url, cache=True):
        path = self._cache_path(url)
        if cache and os.path.isfile(path):
            with open(path, "rb") as f:
                return f.read()
        if cache and os.path.isfile(path + ".404"):
            raise RuntimeError("HTTP Error 404: Not Found (cached)")
        if self.robots is None:
            # fetched with our User-Agent (RobotFileParser.read sends urllib's own one)
            self.robots = urllib.robotparser.RobotFileParser(BASE + "/robots.txt")
            self._wait()
            req = urllib.request.Request(BASE + "/robots.txt", headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=60) as r:
                self.robots.parse(r.read().decode("utf-8", "replace").splitlines())
        if not self.robots.can_fetch(UA, url):
            raise RuntimeError(f"robots.txt disallows {url}")
        for attempt in range(5):
            self._wait()
            req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept-Encoding": "identity"})
            try:
                with urllib.request.urlopen(req, timeout=60) as r:
                    body = r.read()
                    remaining = r.headers.get("x-ratelimit-remaining")
                    if remaining is not None and remaining.strip() in ("0", "1"):
                        time.sleep(float(r.headers.get("x-ratelimit-reset", "10") or 10))
                break
            except urllib.error.HTTPError as e:
                if e.code in (429, 503):
                    delay = float(e.headers.get("Retry-After", "30") or 30)
                    print(f"  {e.code} on {url}; waiting {delay:.0f}s", file=sys.stderr)
                    time.sleep(delay)
                    continue
                if e.code == 404:
                    os.makedirs(self.raw_dir, exist_ok=True)
                    open(path + ".404", "w").close()
                raise
        else:
            raise RuntimeError(f"gave up on {url}")
        os.makedirs(self.raw_dir, exist_ok=True)
        with open(path, "wb") as f:
            f.write(body)
        return body

    def fetched_date(self, url):
        """The date the cached response of `url` was fetched (its file's mtime)."""
        t = os.path.getmtime(self._cache_path(url))
        return datetime.date.fromtimestamp(t).isoformat()

    def _wait(self):
        dt = time.monotonic() - self.last
        if dt < MIN_INTERVAL:
            time.sleep(MIN_INTERVAL - dt)
        self.last = time.monotonic()
        self.sent += 1


# ---- protobuf (schema-less) ----------------------------------------------------------------------

def _varint(b, i):
    r = s = 0
    while True:
        c = b[i]
        i += 1
        r |= (c & 0x7F) << s
        s += 7
        if c < 0x80:
            return r, i


def pb_fields(b):
    i, out = 0, []
    while i < len(b):
        k, i = _varint(b, i)
        f, t = k >> 3, k & 7
        if t == 0:
            v, i = _varint(b, i)
        elif t == 2:
            n, i = _varint(b, i)
            v = b[i:i + n]
            i += n
        elif t == 1:
            v, i = b[i:i + 8], i + 8
        elif t == 5:
            v, i = b[i:i + 4], i + 4
        else:
            raise ValueError(f"wire type {t}")
        out.append((f, t, v))
    return out


def packed(b):
    i, out = 0, []
    while i < len(b):
        v, i = _varint(b, i)
        out.append(v)
    return out


def search_result(body):
    """→ (dictionaries {name: hash}, value lists {name: list}) of a builds search response."""
    inner = pb_fields(pb_fields(body)[0][2])
    dicts, lists = {}, {}
    for f, _, v in inner:
        if f == 6:
            fs = pb_fields(v)
            dicts[fs[0][2].decode()] = fs[1][2].decode()
        elif f == 12:
            fs = pb_fields(v)
            name = fs[0][2].decode()
            strs = [vv.decode() for ff, _, vv in fs if ff == 7]
            ints = [packed(vv) for ff, _, vv in fs if ff == 6]
            rows = [packed(pb_fields(vv)[0][2]) if vv else [] for ff, _, vv in fs if ff == 9]
            dname = next((vv.decode() for ff, _, vv in fs if ff == 11), None)
            lists[name] = {"strs": strs, "ints": ints[0] if ints else [], "rows": rows, "dict": dname}
    return dicts, lists


def ndic(b):
    """poe.ninja's `NDIC` string table: header, block table (string offset, first index) per
    block of `bs` entries, one length byte per entry, then the strings."""
    if b[:4] != b"NDIC":
        raise ValueError("not an NDIC dictionary")
    bs, nb, n = struct.unpack_from("<III", b, 24)
    table = [struct.unpack_from("<II", b, 36 + 8 * k) for k in range(nb)]
    lp = 36 + 8 * nb
    lens, sp = b[lp:lp + n], lp + n
    if sum(lens) != len(b) - sp:
        raise ValueError("NDIC layout")
    out = []
    for soff, idx in table:
        p = sp + soff
        for i in range(idx, min(idx + bs, n)):
            out.append(b[p:p + lens[i]].decode("utf-8"))
            p += lens[i]
    return out


# ---- PoB data (gem tags, for the skill kind) ------------------------------------------------------

def pob_src():
    d = os.path.dirname(os.path.abspath(__file__))
    while d != "/":
        p = os.path.join(d, ".PathOfBuilding", "src")
        if os.path.isdir(p):
            return p
        d = os.path.dirname(d)
    raise RuntimeError(".PathOfBuilding/src not found")


def gem_tags():
    text = open(os.path.join(pob_src(), "Data", "Gems.lua"), encoding="utf-8").read()
    tags = {}
    for m in re.finditer(r'\n\t\tname = "([^"]+)",.*?\n\t\ttags = \{(.*?)\n\t\t\}', text, re.S):
        tags.setdefault(m.group(1), set(re.findall(r"(\w+) = true", m.group(2))))
    return tags


def kind_of(skill, tags):
    t = tags.get(skill, set())
    if skill in DOT_SKILLS:
        return "dot"
    for k in ("minion", "totem", "trap", "mine", "brand"):
        if k in t:
            return "minion" if k == "minion" else "totem/trap/mine"
    if "attack" in t:
        return "attack"
    if "spell" in t:
        return "spell"
    return "other"


def decode_pob(code):
    code = code.strip().replace("-", "+").replace("_", "/")
    raw = base64.b64decode(code + "=" * (-len(code) % 4))
    try:
        return zlib.decompress(raw)
    except zlib.error:
        return zlib.decompress(raw, -15)


# ---- fetch -----------------------------------------------------------------------------------------

def fetch(args):
    out = args.out
    http = Http(os.path.join(out, "raw"))
    os.makedirs(os.path.join(out, "meta"), exist_ok=True)
    today = datetime.date.today().isoformat()
    index = json.loads(http.get(f"{BASE}/poe1/api/data/index-state", cache=not args.refresh))
    snaps = {s["url"]: s for s in index["snapshotVersions"] if s["type"] == "exp"}
    tags = gem_tags()
    dict_cache = {}

    def dictionary(h):
        if h not in dict_cache:
            dict_cache[h] = ndic(http.get(f"{BASE}/poe1/api/builds/dictionary/{h}?language=en"))
        return dict_cache[h]

    picked_skills, kinds = {}, {}
    picks = []
    for asc in ASCENDANCIES:
        plan = LEAGUES[: args.per_class]
        used_rows = set()
        for slot, league in enumerate(plan):
            cands = []
            for lg in [league] + [x for x in dict.fromkeys(LEAGUES) if x != league]:
                snap = snaps[lg]
                q = urllib.parse.urlencode({"overview": snap["snapshotName"], "type": "exp", "class": asc})
                body = http.get(f"{BASE}/poe1/api/builds/{snap['version']}/search?{q}")
                dicts, lists = search_result(body)
                names, accounts = lists["name"]["strs"], lists["account"]["strs"]
                classes = dictionary(dicts["class"])
                cls = [classes[i] for i in lists["class"]["ints"]]
                gems = dictionary(dicts["gem"])
                dps_skill = lists.get("dps.skill", {}).get("ints", [])
                skills_rows = lists.get("skills", {}).get("rows", [])
                for r, (n, a) in enumerate(zip(names, accounts)):
                    if cls[r] != asc or (lg, a, n) in used_rows:
                        continue
                    sk = None
                    if r < len(dps_skill):
                        sk = gems[dps_skill[r]]
                    if not sk and r < len(skills_rows) and skills_rows[r]:
                        sk = gems[skills_rows[r][0]]
                    if sk:
                        cands.append((lg, snap, r, a, n, sk))
                if cands:
                    break
            if not cands:
                print(f"{asc} {league}: no candidates", file=sys.stderr)
                continue

            def score(c):
                k = kind_of(c[5], tags)
                return (
                    (c[5] not in picked_skills) * 4
                    + (kinds.get(k, 0) < min(kinds.values(), default=0) + 3) * 2
                    + (k not in {kind_of(p["skill"], tags) for p in picks if p["class"] == asc}),
                    -c[2],
                )

            ranked = sorted(cands, key=score, reverse=True)
            lg, snap, r, a, n, sk = ranked[0]
            used_rows.add((lg, a, n))
            picked_skills[sk] = picked_skills.get(sk, 0) + 1
            kinds[kind_of(sk, tags)] = kinds.get(kind_of(sk, tags), 0) + 1
            picks.append({"class": asc, "league": lg, "version": snap["version"], "tree": snap["passiveTree"],
                          "overview": snap["snapshotName"], "rank": r + 1, "account": a, "name": n, "skill": sk,
                          # fall-backs when the character page is gone (404): the next-best rows
                          "alts": [{"league": c[0], "version": c[1]["version"], "tree": c[1]["passiveTree"],
                                    "overview": c[1]["snapshotName"], "rank": c[2] + 1, "account": c[3],
                                    "name": c[4], "skill": c[5]} for c in ranked[1:4]]})
    print(f"picked {len(picks)}: kinds {kinds}", file=sys.stderr)
    stem_n = 0
    for p0 in picks:
        stem = f"fresh_{stem_n:03d}"
        stem_n += 1
        meta_path = os.path.join(out, "meta", stem + ".json")
        alts = p0.pop("alts")
        failed = []
        for p in [p0] + [dict(p0, **alt) for alt in (alts if args.alternates else [])]:
            q = urllib.parse.urlencode({"account": p["account"], "name": p["name"], "overview": p["overview"], "type": "exp"})
            api = f"{BASE}/poe1/api/builds/{p['version']}/character?{q}"
            page = (f"{BASE}/poe1/builds/{p['league']}/character/{urllib.parse.quote(p['account'])}/"
                    f"{urllib.parse.quote(p['name'])}")
            try:
                ch = json.loads(http.get(api))
                code = ch.get("pathOfBuildingExport") or ""
                if not code:
                    raise RuntimeError("no pathOfBuildingExport")
                xml = decode_pob(code)
                break
            except Exception as e:  # noqa: BLE001 - reported per build
                print(f"{stem}: {p['class']} {p['name']}: {e}", file=sys.stderr)
                failed.append({"source": page, "error": str(e)})
        else:
            p0.update(stem=stem, error=failed[0]["error"], source=failed[0]["source"], fetched=today, failed=failed)
            p0.pop("account", None)
            json.dump(p0, open(meta_path, "w"), indent=1)
            continue
        dest = os.path.join(out, stem + ".xml")
        dropped = os.path.join(out, "dropped", stem + ".xml")
        if not os.path.isfile(dropped):
            with open(dest, "wb") as f:
                f.write(xml)
        p.update(stem=stem, source=page, fetched=http.fetched_date(api), failed=failed, sha256=hashlib.sha256(xml).hexdigest(),
                 level=ch.get("level"), ascendancy=ch.get("ascendancyClassName"), baseClass=ch.get("baseClass"),
                 kind=kind_of(p["skill"], tags), bytes=len(xml))
        p.pop("account", None)  # the source URL already names it
        json.dump(p, open(meta_path, "w"), indent=1)
        print(f"{stem}: {p['league']} {p['class']} {p['skill']} ({len(xml)} bytes)", file=sys.stderr)
    print(f"{http.sent} requests sent", file=sys.stderr)


# ---- manifest --------------------------------------------------------------------------------------

def manifest(args):
    out = args.out
    metas = []
    for f in sorted(os.listdir(os.path.join(out, "meta"))):
        if f.endswith(".json"):
            metas.append(json.load(open(os.path.join(out, "meta", f))))
    kept = {f[:-4] for f in os.listdir(out) if f.endswith(".xml")}
    drop_dir = os.path.join(out, "dropped")
    dropped = {}
    if os.path.isdir(drop_dir):
        for f in os.listdir(drop_dir):
            if f.endswith(".reason"):
                dropped[f[:-7]] = open(os.path.join(drop_dir, f)).read().strip()
    tags = gem_tags()

    def pob_kind(m):
        k = kind_of(m["skill"], tags)
        if not args.probe:
            return k
        try:
            ca = json.load(open(os.path.join(args.probe, m["stem"], "ca.json")))["main"]
        except (OSError, ValueError, KeyError):
            return k
        hit = max(ca.get("TotalDPS") or 0, ca.get("CombinedDPS") or 0)
        dot = ca.get("TotalDot") or ca.get("TotalDotDPS") or 0
        if k != "minion" and dot and dot > 0.5 * (ca.get("CombinedDPS") or hit or 1):
            return k + "+dot" if k not in ("dot",) else k
        return k

    p = print
    p("# INT-1-FRESH corpus manifest")
    p()
    p("Fresh parity corpus for calc doc §11.1 row INT-1 (the \"100 fresh poe.ninja builds\" half).")
    p("Fetched with `scripts/port/fetch_fresh_corpus.py fetch` from poe.ninja's public build pages")
    p("(`/poe1/api/builds/<version>/search` per ascendancy, then each character's")
    p("`pathOfBuildingExport`). The XML lives in `.parity/fresh/` (gitignored, never committed);")
    p("`CA_CORPUS_DIR=.parity/fresh` runs the CA harness on it. Triage: `INT-1-fresh-triage.md`.")
    p("This file is generated: `scripts/port/fetch_fresh_corpus.py manifest --probe target/tmp/ca-probe`.")
    p()
    ok = [m for m in metas if m["stem"] in kept]
    p(f"**Fetched {len(metas)}, kept {len(ok)}, dropped {len(metas) - len(ok)}.**")
    p()
    p("## Selection")
    p()
    p("- Source: poe.ninja's own API (the JSON/protobuf the build pages load), not pobb.in. Leagues:")
    p("  Standard (3.29 tree), Hardcore Allflame and SSF Allflame (3.29), Mirage (3.28). Allflame")
    p("  softcore trade is left out: the all57 corpus already comes from it.")
    p(f"- {len(ASCENDANCIES)} ascendancies x 5 picks (Standard, HC Allflame, SSF Allflame, Mirage,")
    p("  Standard), from the first 100 characters of each class search. Each pick prefers a main")
    p("  skill not picked yet and a skill kind (attack, spell, minion, totem/trap/mine/brand, DoT) that")
    p("  is under-represented. A pick whose character API answers 404 falls back to the next 3 rows")
    p("  (`--alternates`).")
    p("- Politeness: at most one request per second, User-Agent `pob-port-parity-corpus/1.0`,")
    p("  robots.txt (only `/dev/` is disallowed) checked, `Retry-After` honoured (one 429 with")
    p("  `Retry-After: 3600` was hit and waited out). The two complete fetch runs sent 213 requests;")
    p("  about 20 more went to probing the API and to two runs that stopped early.")
    p("- `sha256` is the first 16 hex digits of the decoded XML's SHA-256. `kind` is the main skill's")
    p("  kind from PoB's gem tags (`other`: not a gem, e.g. an item or ascendancy skill; `+dot`: PoB's")
    p("  `TotalDot` is over half of `CombinedDPS`).")
    p()
    p("## PoB checks")
    p()
    p("- Every kept build loads in the oracle (PoB v2.67.2 under LuaJIT, the CA harness's `pob_run`)")
    p("  without a PoB error: no `error.txt`, no `promptMsg`.")
    dc = os.path.join(out, "datacheck.txt")
    if os.path.isfile(dc):
        for line in open(dc):
            p(line.rstrip("\n"))
    p()
    p("## Builds")
    p()
    p("| stem | league | tree | class | main skill (poe.ninja) | kind | level | fetched | sha256 | source |")
    p("|---|---|---|---|---|---|---|---|---|---|")
    for m in ok:
        p(f"| {m['stem']} | {m['league']} | {m['tree'].replace('PassiveTree-', '')} | {m['class']} | {m['skill']} | "
          f"{pob_kind(m)} | {m.get('level', '')} | {m['fetched']} | `{m['sha256'][:16]}` | [poe.ninja]({m['source']}) |")
    p()
    p("## Dropped")
    p()
    p("| stem | league | class | main skill | reason | source |")
    p("|---|---|---|---|---|---|")
    for m in metas:
        if m["stem"] in kept:
            continue
        why = dropped.get(m["stem"])
        if not why and m.get("error"):
            n = len(m.get("failed") or [None])
            why = (f"fetch: poe.ninja character API {m['error'].replace(' (cached)', '')}"
                   + (f" for the pick and its {n - 1} fall-backs" if n > 1 else ""))
        p(f"| {m['stem']} | {m['league']} | {m['class']} | {m['skill']} | {why} | [poe.ninja]({m['source']}) |")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cmd", choices=["fetch", "manifest"])
    root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    default_out = None
    d = root
    while d != "/":
        if os.path.isdir(os.path.join(d, ".parity")):
            default_out = os.path.join(d, ".parity", "fresh")
            break
        d = os.path.dirname(d)
    ap.add_argument("--out", default=default_out)
    ap.add_argument("--per-class", type=int, default=5)
    ap.add_argument("--alternates", action="store_true",
                    help="replace a pick whose character page is gone (404) by the next-best row")
    ap.add_argument("--refresh", action="store_true", help="refetch index-state (new snapshot versions)")
    ap.add_argument("--probe", help="the CA test's PoB cache dir (target/tmp/ca-probe)")
    args = ap.parse_args()
    if not args.out:
        sys.exit("no .parity directory found; pass --out")
    (fetch if args.cmd == "fetch" else manifest)(args)


if __name__ == "__main__":
    main()
