#!/usr/bin/env bash
# flock-serialised cargo for the port (calc doc D20, §10.1; AMENDMENTS A5).
#
# Usage: scripts/port/cargo.sh <cargo args...>      e.g. scripts/port/cargo.sh check -p pob-calc
#
# - Exactly one cargo invocation runs at a time machine-wide, across all worktrees: the
#   whole cargo run holds an exclusive flock on $POB_PORT_LOCK_DIR/cargo.lock
#   (default: ${XDG_RUNTIME_DIR:-/tmp}/pob-port).
# - Exports CARGO_BUILD_JOBS=3 and RUSTC_WRAPPER=$HOME/.cargo/bin/sccache (when executable;
#   an existing RUSTC_WRAPPER is kept).
# - CARGO_TARGET_DIR defaults to <worktree>/target: never share a target dir between
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

export CARGO_BUILD_JOBS=3
if [ -z "${RUSTC_WRAPPER:-}" ] && [ -x "$HOME/.cargo/bin/sccache" ]; then
    export RUSTC_WRAPPER="$HOME/.cargo/bin/sccache"
fi

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
worktree=$(git -C "$script_dir" rev-parse --show-toplevel 2>/dev/null || (cd "$script_dir/../.." && pwd -P))
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$worktree/target}"

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

lock_dir="${POB_PORT_LOCK_DIR:-${XDG_RUNTIME_DIR:-/tmp}/pob-port}"
mkdir -p "$lock_dir"
lock="$lock_dir/cargo.lock"

exec 9>>"$lock"
if ! flock -n 9; then
    echo "cargo.sh: waiting for the cargo lock ($lock)..." >&2
    flock 9
fi
here=$(pwd -P)
case "$here/" in
    "$worktree"/*) ;;
    *) cd "$worktree" ;;
esac
cargo "$@"
