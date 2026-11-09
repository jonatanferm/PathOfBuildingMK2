#!/usr/bin/env bash
# Userspace OOM guard for the port's agent runs (2026-11-09 incident: a kernel OOM kill inside the
# terminal's systemd scope made systemd stop the whole scope, agent session included).
# When MemAvailable drops below POB_OOMGUARD_MIN_MB (default 1200), SIGKILL the largest-RSS
# build/test process (test binaries under target/, rustc, pob_oracle, luajit). A userspace
# SIGKILL is not an OOM kill, so the scope survives. Run it as its own unit:
#   systemd-run --user --unit=pob-port-oomguard scripts/port/oomguard.sh
set -u
min_mb="${POB_OOMGUARD_MIN_MB:-1200}"
pat='/target/(debug|release)/|(^|/)rustc( |$)|pob_oracle|luajit|/deps/[A-Za-z0-9_]+-[0-9a-f]{16}'
while true; do
    avail_kb=$(awk '/^MemAvailable:/{print $2}' /proc/meminfo)
    if [ "$avail_kb" -lt $((min_mb * 1024)) ]; then
        victim=$(ps -eo pid=,rss=,args= --sort=-rss | grep -E "$pat" | grep -v -E 'grep|oomguard' | head -1)
        if [ -n "$victim" ]; then
            pid=$(echo "$victim" | awk '{print $1}')
            echo "$(date '+%F %T') avail=${avail_kb}kB killing: $victim" | cut -c1-300
            kill -9 "$pid" 2>/dev/null
            sleep 3
        fi
    fi
    sleep 1
done
