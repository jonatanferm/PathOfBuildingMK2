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
# - Refuses `test --workspace` / `test --all` and release builds without -p (machine budget).
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

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
worktree=$(git -C "$script_dir" rev-parse --show-toplevel 2>/dev/null || (cd "$script_dir/../.." && pwd))
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$worktree/target}"

if [ "${POB_PORT_ALLOW_WORKSPACE:-0}" != 1 ]; then
    sub="$1"
    has_p=0
    for a in "$@"; do
        case "$a" in
            --workspace|--all)
                if [ "$sub" = test ] || [ "$sub" = t ]; then
                    echo "cargo.sh: refusing '$sub $a' (never test the whole workspace; use -p <crate>)" >&2
                    exit 2
                fi ;;
            -p|--package|-p*|--package=*) has_p=1 ;;
        esac
    done
    for a in "$@"; do
        case "$a" in
            --release|-r)
                if [ "$has_p" = 0 ] && { [ "$sub" = build ] || [ "$sub" = b ] || [ "$sub" = test ] || [ "$sub" = t ]; }; then
                    echo "cargo.sh: refusing a release '$sub' without -p (never release-build the workspace)" >&2
                    exit 2
                fi ;;
        esac
    done
fi

lock_dir="${POB_PORT_LOCK_DIR:-${XDG_RUNTIME_DIR:-/tmp}/pob-port}"
mkdir -p "$lock_dir"
lock="$lock_dir/cargo.lock"

exec 9>>"$lock"
if ! flock -n 9; then
    echo "cargo.sh: waiting for the cargo lock ($lock)..." >&2
    flock 9
fi
cd "$worktree"
cargo "$@"
