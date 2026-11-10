#!/usr/bin/env bash
# flock-serialised cargo for the port (calc doc D20, §10.1; AMENDMENTS A5).
#
# Usage: scripts/port/cargo.sh <cargo args...>      e.g. scripts/port/cargo.sh check -p pob-calc
#
# - At most $POB_PORT_SLOTS (default 2) cargo invocations run at a time machine-wide, across all worktrees: the
#   whole cargo run holds an exclusive flock on $POB_PORT_LOCK_DIR/cargo.lock
#   (default: ${XDG_RUNTIME_DIR:-/tmp}/pob-port).
# - Exports CARGO_BUILD_JOBS=${POB_PORT_JOBS:-3} and RUSTC_WRAPPER=$HOME/.cargo/bin/sccache (when executable;
#   an existing RUSTC_WRAPPER is kept).
# - The target dir is cargo's default <worktree>/target (not exported, see below): never share a target dir between
#   worktrees (path-based fingerprints thrash).
# - cargo runs in the caller's directory when that is inside this worktree, so relative
#   paths such as --manifest-path resolve as the caller wrote them. From anywhere else it
#   runs in the worktree root, so the target dir always belongs to the tree it builds.
# - Machine-budget guards (D20, §10.1). The root manifest is virtual and has no
#   default-members, so a bare `cargo test` at the root is `cargo test --workspace`.
#   The guards therefore require an explicit package selection, not just the absence of flags:
#     * test/t/bench need -p/--package and must not pass --workspace/--all;
#     * a release-profile build/b/test/t/bench/check/c (--release, -r, --profile release|bench;
#       bench always is one) needs -p and must not pass --workspace/--all.
#   POB_PORT_ALLOW_WORKSPACE=1 overrides.
set -euo pipefail

if [ "$#" -eq 0 ]; then
    echo "usage: $0 <cargo args...>" >&2
    exit 2
fi

export CARGO_BUILD_JOBS="${POB_PORT_JOBS:-3}"
if [ -z "${RUSTC_WRAPPER:-}" ] && [ -x "$HOME/.cargo/bin/sccache" ]; then
    export RUSTC_WRAPPER="$HOME/.cargo/bin/sccache"
fi

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
worktree=$(git -C "$script_dir" rev-parse --show-toplevel 2>/dev/null || (cd "$script_dir/../.." && pwd -P))
# Do NOT export CARGO_TARGET_DIR: sccache hashes CARGO_* env vars, so a per-worktree value made
# every worktree miss the cache for every crate (measured 2026-11-09: 1.6% Rust hit rate).
# cargo's default target dir is already <worktree>/target because we run inside the worktree.

refuse() {
    echo "cargo.sh: refusing $1" >&2
    exit 2
}

if [ "${POB_PORT_ALLOW_WORKSPACE:-0}" != 1 ]; then
    # The subcommand is the first non-option argument (skipping +toolchain and the values
    # of the cargo global options that take one).
    sub=""
    skip=0
    for a in "$@"; do
        if [ "$skip" = 1 ]; then skip=0; continue; fi
        case "$a" in
            +*) ;;
            --config|-Z|-C|--color|--explain) skip=1 ;;
            -*) ;;
            *) sub="$a"; break ;;
        esac
    done

    has_p=0; has_ws=0; release=0
    if [ "$sub" = bench ]; then release=1; fi
    prev=""
    for a in "$@"; do
        if [ "$a" = -- ]; then break; fi    # the rest goes to the test binary
        case "$a" in
            --workspace|--all) has_ws=1 ;;
            -p|--package|-p?*|--package=*) has_p=1 ;;
            --release|-r|--profile=release|--profile=bench) release=1 ;;
        esac
        if [ "$prev" = --profile ] && { [ "$a" = release ] || [ "$a" = bench ]; }; then release=1; fi
        prev="$a"
    done

    case "$sub" in
        test|t|bench)
            if [ "$has_ws" = 1 ]; then
                refuse "'$sub --workspace/--all' (never test the whole workspace; use -p <crate>)"
            fi
            if [ "$has_p" = 0 ]; then
                refuse "'$sub' without -p (the root manifest is virtual, so this is '$sub --workspace'; use -p <crate>)"
            fi ;;
    esac
    if [ "$release" = 1 ]; then
        case "$sub" in
            build|b|test|t|bench|check|c)
                if [ "$has_ws" = 1 ] || [ "$has_p" = 0 ]; then
                    refuse "a release-profile '$sub' without -p or with --workspace/--all (never release-build the workspace)"
                fi ;;
        esac
    fi
fi

sub="${sub:-}"   # set by the guard block above (empty with POB_PORT_ALLOW_WORKSPACE=1)
lock_dir="${POB_PORT_LOCK_DIR:-${XDG_RUNTIME_DIR:-/tmp}/pob-port}"
mkdir -p "$lock_dir"

# take_slot FD PREFIX COUNT LABEL: hold one of COUNT flocks $lock_dir/PREFIX.lock[.i] on FD.
# Compile slots (fd 9, prefix "cargo", POB_PORT_SLOTS, default 2) are held while cargo compiles.
# For test/bench the slot is released after `--no-run`, and the test binaries then run under a run
# slot (fd 8, prefix "run", POB_PORT_RUN_SLOTS, default 3), so long oracle runs no longer block
# other agents' compiles (audit 2026-11-09: 15.5 min slot wait for a 69 s build).
take_slot() {
    local fd="$1" prefix="$2" n="$3" label="$4" waited=0 i l
    while :; do
        for i in $(seq 0 $((n - 1))); do
            if [ "$i" = 0 ]; then l="$lock_dir/$prefix.lock"; else l="$lock_dir/$prefix.lock.$i"; fi
            eval "exec $fd>>\"\$l\""
            if flock -n "$fd"; then return 0; fi
            eval "exec $fd>&-"
        done
        if [ "$waited" = 0 ]; then echo "cargo.sh: waiting for one of $n $label slots ($lock_dir)..." >&2; waited=1; fi
        sleep 2
    done
}

here=$(pwd -P)
case "$here/" in
    "$worktree"/*) ;;
    *) cd "$worktree" ;;
esac

# Worktrees on a RAM-backed /tmp (reviewers' /tmp/review-*) build into ~/.cache/pob-port-targets/
# instead: target dirs are 2-3 GB each and filled the 7.6 GB tmpfs. --target-dir is passed as an
# argument (an exported CARGO_TARGET_DIR would bust sccache). Dirs whose worktree is gone are removed.
args=("$@")
tcache="$HOME/.cache/pob-port-targets"
case "$worktree" in
    /tmp/*|/dev/shm/*)
        case "$sub" in
            build|b|check|c|test|t|bench|run|r|clippy|doc|d|rustc|fix)
                td="$tcache/$(basename "$worktree")"
                mkdir -p "$td"; echo "$worktree" > "$td/.worktree"
                new=(); done_ins=0
                for a in "${args[@]}"; do
                    new+=("$a")
                    if [ "$done_ins" = 0 ] && [ "$a" = "$sub" ]; then new+=(--target-dir "$td"); done_ins=1; fi
                done
                args=("${new[@]}") ;;
        esac ;;
esac
if [ -d "$tcache" ]; then
    for d in "$tcache"/*/; do
        [ -f "$d.worktree" ] && [ ! -d "$(cat "$d.worktree")" ] && rm -rf "$d"
    done
fi

# Merge-time tests use the quick corpus: in the MAIN checkout (only integrators and the orchestrator
# run tests there) `cargo.sh test` points $POB_PARITY_DIR at .parity-quick (14 of the 57 builds,
# scripts/port/quick_parity.sh). Worktrees (implementers, reviewers) keep the full corpus, and the
# full suite runs every few hours (scripts/port/full_suite.sh). POB_FULL_SUITE=1 forces the full corpus.
if [ -d "$worktree/.git" ] && [ -z "${POB_PARITY_DIR:-}" ] && [ "${POB_FULL_SUITE:-0}" != 1 ]; then
    case "${sub:-}" in
        test|t)
            if [ -d "$worktree/.parity-quick/builds" ]; then
                export POB_PARITY_DIR="$worktree/.parity-quick" POB_QUICK_CORPUS=1
                echo "cargo.sh: main checkout -> quick corpus ($(ls "$POB_PARITY_DIR/builds" | wc -l) builds, POB_PARITY_DIR=$POB_PARITY_DIR); POB_FULL_SUITE=1 for all builds" >&2
            fi ;;
    esac
fi

# Each cargo run (compiles and the test binaries it starts) gets its own memory-capped systemd
# scope, so an OOM kills only this run instead of the whole terminal scope (2026-11-09 incident:
# a 6 GB test binary got the terminal scope, and with it the agent session, stopped).
use_scope=0
if [ "${POB_PORT_NO_SCOPE:-0}" != 1 ] && command -v systemd-run >/dev/null 2>&1 \
    && systemd-run --user --scope --quiet true >/dev/null 2>&1; then
    use_scope=1
fi
run_cargo() {
    if [ "$use_scope" = 1 ]; then
        systemd-run --user --scope --quiet -p MemoryMax="${POB_PORT_MEM:-5G}" -p MemorySwapMax="${POB_PORT_SWAP:-1G}" cargo "$@"
    else
        cargo "$@"
    fi
}

two_phase=0
case "$sub" in
    test|t|bench)
        two_phase=1
        for a in "${args[@]}"; do
            [ "$a" = -- ] && break
            [ "$a" = --no-run ] && two_phase=0
        done ;;
esac

case "$sub" in
    metadata|tree|locate-project|pkgid|version|help|search|read-manifest)
        exec cargo "${args[@]}" ;;   # no compile: no slot
esac
take_slot 9 cargo "${POB_PORT_SLOTS:-2}" compile
if [ "$two_phase" = 0 ]; then
    run_cargo "${args[@]}"
    exit $?
fi
# Phase 1: compile under the compile slot (--no-run goes before any `--`).
pre=(); post=(); seen=0
for a in "${args[@]}"; do
    if [ "$seen" = 0 ] && [ "$a" = -- ]; then seen=1; fi
    if [ "$seen" = 0 ]; then pre+=("$a"); else post+=("$a"); fi
done
run_cargo "${pre[@]}" --no-run || exit $?
exec 9>&-
# Phase 2: run the (now fresh) test binaries under a run slot.
take_slot 8 run "${POB_PORT_RUN_SLOTS:-3}" test-run
run_cargo "${args[@]}"
exit $?
