#!/usr/bin/env bash
# Serialised LuaJIT oracle (calc-architecture.md D20, §9.2, §9.5).
#
# Every PoB-under-LuaJIT process of the port goes through here: a flock on
# $POB_ORACLE_LOCK (default /tmp/pob-port-oracle.lock, shared by every
# worktree and agent on the machine) keeps one oracle process (~400 MB)
# alive at a time. pob_oracle takes the same lock itself when run directly;
# under this script it is told the lock is held (POB_ORACLE_LOCK_HELD=1).
#
# Usage:
#   scripts/port/oracle.sh run --build X.xml [--stage HIT[,HIT...]] [--stages F]
#       probe one build (default selection, or the given hits / globs) and
#       store the dumps in .parity/probe-cache/<sha>/<build-blake3>/
#   scripts/port/oracle.sh nondet [--verify] [--max-runs N] [--cache] [--keep-dir DIR] [--fresh] [build.xml ...]
#       §9.5 two (three) runs per build -> .parity/nondet/<stem>.json
#       (default: every .parity/builds/*.xml; --fresh: every .parity/fresh/*.xml, the
#       INT-1-FRESH corpus, whose fresh_NNN stems share .parity/nondet); --verify always runs a third
#       time and reports differences the first pair did not record;
#       --max-runs N keeps running (up to N) while a run still adds to the record;
#       --cache stores run 1 in the probe cache, --keep-dir keeps it as
#       DIR/<stem>/ (outputs.json, hits.json, provenance.json, state/)
#   scripts/port/oracle.sh corpus [build.xml ...]
#       probe every build into .parity/probe/<stem>/ and the probe cache
#   scripts/port/oracle.sh patch [--stages F]      print the patched src/
#   scripts/port/oracle.sh exec <pob_oracle args>  any pob_oracle command
#
# Env: POB_ORACLE_BIN (skip the cargo build), POB_PARITY_DIR, POB_SRC,
#      ORACLE_STAGES (default docs/port/stages.toml when it exists).
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
lock="${POB_ORACLE_LOCK:-/tmp/pob-port-oracle.lock}"
cmd="${1:-}"
[ -n "$cmd" ] || { awk 'NR > 1 && !/^#/ { exit } NR > 1 { print }' "$0" >&2; exit 2; }
shift

parity="${POB_PARITY_DIR:-}"
if [ -z "$parity" ]; then
    # from a git worktree the gitignored corpus lives in an enclosing checkout
    d="$root"
    while [ "$d" != "/" ]; do
        if [ -d "$d/.parity/builds" ]; then parity="$d/.parity"; break; fi
        d="$(dirname "$d")"
    done
fi

bin="${POB_ORACLE_BIN:-}"
if [ -z "$bin" ]; then
    if [ -x "$root/scripts/port/cargo.sh" ]; then
        (cd "$root" && scripts/port/cargo.sh build --release -q -p pob-oracle --bin pob_oracle)
    else
        export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-3}"
        if [ -x "$HOME/.cargo/bin/sccache" ]; then export RUSTC_WRAPPER="${RUSTC_WRAPPER:-$HOME/.cargo/bin/sccache}"; fi
        (cd "$root" && cargo build --release -q -p pob-oracle --bin pob_oracle)
    fi
    bin="${CARGO_TARGET_DIR:-$root/target}/release/pob_oracle"
fi

stages="${ORACLE_STAGES:-}"
if [ -z "$stages" ] && [ -f "$root/docs/port/stages.toml" ]; then stages="$root/docs/port/stages.toml"; fi

locked() {
    flock "$lock" env POB_ORACLE_LOCK_HELD=1 "$@"
}

need_parity() {
    [ -n "$parity" ] || { echo "no .parity/builds found (set POB_PARITY_DIR)" >&2; exit 1; }
}

stage_args=()

# The patched src/ for "${stage_args[@]}" (the patcher's log shown on failure).
patch_src() {
    local log rc=0 out
    log="$(mktemp "${TMPDIR:-/tmp}/oracle_patch.XXXXXX")"
    out="$("$bin" patch "${stage_args[@]}" 2> "$log")" || rc=$?
    if [ "$rc" -ne 0 ] || [ -z "$out" ]; then
        echo "oracle.sh: pob_oracle patch failed (exit $rc):" >&2
        cat "$log" >&2
        rm -f "$log"
        exit 1
    fi
    rm -f "$log"
    printf '%s\n' "$out"
}

case "$cmd" in
    run)
        build="" select=""
        while [ $# -gt 0 ]; do
            case "$1" in
                --build) build="$2"; shift 2 ;;
                --stage) select="$2"; shift 2 ;;
                --stages) stages="$2"; shift 2 ;;
                *) echo "oracle.sh run: unexpected $1" >&2; exit 2 ;;
            esac
        done
        [ -n "$build" ] || { echo "oracle.sh run needs --build X.xml" >&2; exit 2; }
        need_parity
        if [ -n "$stages" ]; then stage_args=(--stages "$stages"); fi
        sel=()
        if [ -n "$select" ]; then sel=(--select "$select"); fi
        out="$(mktemp -d "${TMPDIR:-/tmp}/oracle_run.XXXXXX")"
        trap 'rm -rf "$out"' EXIT
        POB_PARITY_DIR="$parity" locked "$bin" probe "$build" --out-dir "$out" --cache "${stage_args[@]}" "${sel[@]}"
        ;;
    nondet)
        extra=()
        keep_dir=""
        builds=()
        while [ $# -gt 0 ]; do
            case "$1" in
                --verify) extra+=(--verify); shift ;;
                --max-runs) extra+=(--max-runs "$2"); shift 2 ;;
                --cache) extra+=(--cache); shift ;;
                --keep-dir) keep_dir="$2"; shift 2 ;;
                --stages) stages="$2"; shift 2 ;;
                --fresh) fresh=1; shift ;;
                *) builds+=("$1"); shift ;;
            esac
        done
        need_parity
        if [ "${fresh:-}" = 1 ]; then builds+=("$parity"/fresh/*.xml); fi
        if [ ${#builds[@]} -eq 0 ]; then builds=("$parity"/builds/*.xml); fi
        if [ -n "$stages" ]; then stage_args=(--stages "$stages"); fi
        # patch once (outside the lock: no LuaJIT PoB involved)
        src="$(patch_src)"
        mkdir -p "$parity/nondet"
        for xml in "${builds[@]}"; do
            stem="$(basename "$xml" .xml)"
            keep=()
            if [ -n "$keep_dir" ]; then keep=(--keep "$keep_dir/$stem"); fi
            POB_PARITY_DIR="$parity" locked "$bin" nondet "$xml" --patched "$src" "${extra[@]}" "${keep[@]}" \
                --out "$parity/nondet/$stem.json" 2> "$parity/nondet/$stem.log" \
                || echo "{\"build\":\"$stem\",\"error\":\"see $parity/nondet/$stem.log\"}"
        done
        ;;
    corpus)
        builds=()
        while [ $# -gt 0 ]; do
            case "$1" in
                --stages) stages="$2"; shift 2 ;;
                *) builds+=("$1"); shift ;;
            esac
        done
        need_parity
        if [ ${#builds[@]} -eq 0 ]; then builds=("$parity"/builds/*.xml); fi
        if [ -n "$stages" ]; then stage_args=(--stages "$stages"); fi
        src="$(patch_src)"
        for xml in "${builds[@]}"; do
            stem="$(basename "$xml" .xml)"
            dir="$parity/probe/$stem"
            rm -rf "$dir"
            mkdir -p "$dir"
            if POB_PARITY_DIR="$parity" locked "$bin" probe "$xml" --out-dir "$dir" --patched "$src" --cache 2> "$dir/log.txt"; then
                printf '%s\tok\n' "$stem"
            else
                printf '%s\tFAIL\n' "$stem"
            fi
        done
        ;;
    patch)
        while [ $# -gt 0 ]; do
            case "$1" in
                --stages) stages="$2"; shift 2 ;;
                *) echo "oracle.sh patch: unexpected $1" >&2; exit 2 ;;
            esac
        done
        if [ -n "$stages" ]; then stage_args=(--stages "$stages"); fi
        "$bin" patch "${stage_args[@]}"
        ;;
    exec)
        locked "$bin" "$@"
        ;;
    *)
        echo "oracle.sh: unknown command $cmd" >&2
        exit 2
        ;;
esac
