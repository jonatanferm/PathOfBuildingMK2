#!/bin/bash
# RUSTC_WRAPPER that records the peak RSS (wait4 ru_maxrss, linker child included) of every rustc
# of a workspace crate (`pob_*`) and of every `--test` crate (calc-split-plan §1.3/§5.3, CCR-0131).
# Those run directly under calc_ptime.py (with sccache as the wrapper, rustc runs inside the sccache
# server and its memory is invisible); every other crate goes to sccache as usual.
#   CSPLIT_LOG=/abs/rustc.log CSPLIT_TAG=cold RUSTC_WRAPPER=$PWD/scripts/port/calc_rustc_wrap.sh \
#       scripts/port/cargo.sh build -p pob-calc --lib
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
rustc="$1"; shift
crate=""; prev=""; is_test=0
for a in "$@"; do [ "$prev" = --crate-name ] && crate="$a"; [ "$a" = --test ] && is_test=1; prev="$a"; done
LOG="${CSPLIT_LOG:-${XDG_RUNTIME_DIR:-/tmp}/calc_rustc.log}"
if [ "$is_test" = 1 ] || [[ "$crate" == pob_* ]]; then
    exec python3 "$here/calc_ptime.py" "$LOG" "rustc:$crate test=$is_test tag=${CSPLIT_TAG:-}" -- "$rustc" "$@"
fi
if [ -x "$HOME/.cargo/bin/sccache" ]; then
    exec "$HOME/.cargo/bin/sccache" "$rustc" "$@"
fi
exec "$rustc" "$@"
