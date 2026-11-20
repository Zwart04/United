#!/usr/bin/env bash
# Memory and cold-start benchmark for the native binary.
#
# Samples /proc/<pid>/status so the numbers come from the kernel, not from the
# allocator. Run after a release build:
#
#   ./tools/bench.sh [binary] [data-dir] [seconds]
#
# Prints a Markdown table; paste the result into docs/RUST.md.
set -euo pipefail

BIN=${1:-target/release/united}
DATA=${2:-$(mktemp -d)}
SECONDS_TO_WATCH=${3:-15}
INTERVAL=0.25

[ -x "$BIN" ] || { echo "missing binary: $BIN" >&2; exit 1; }
mkdir -p "$DATA"

sample() { # field from /proc/<pid>/status, e.g. VmRSS
    awk -v key="$1" '$1 == key ":" { print $2; exit }' /proc/$PID/status 2>/dev/null || true
}
cpu_ticks() {
    awk '{ print $14 + $15 }' /proc/$PID/stat 2>/dev/null || echo 0
}
threads() {
    awk '/^Threads:/ { print $2 }' /proc/$PID/status 2>/dev/null || echo 0
}
fds() { ls /proc/$PID/fd 2>/dev/null | wc -l; }

echo "binary: $BIN ($(stat -c %s "$BIN" | awk '{ printf "%.1f MB", $1/1048576 }'))"
echo "data:   $DATA"

start_ns=$(date +%s%N)
UNITED_DATA_DIR="$DATA" "$BIN" >/tmp/opencode-united-bench.log 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true' EXIT

# Startup: wait until the process stops burning CPU for three samples in a row.
last=-1
steady=0
ready_ms=""
while kill -0 $PID 2>/dev/null; do
    now=$(cpu_ticks)
    if [ "$now" = "$last" ]; then
        steady=$((steady + 1))
    else
        steady=0
        last=$now
    fi
    if [ "$steady" -ge 3 ]; then
        ready_ms=$(( ($(date +%s%N) - start_ns) / 1000000 ))
        break
    fi
    sleep $INTERVAL
done

rss_values=()
for _ in $(seq 1 $((SECONDS_TO_WATCH * 4))); do
    kill -0 $PID 2>/dev/null || break
    rss=$(sample VmRSS)
    [ -n "$rss" ] && rss_values+=("$rss")
    sleep $INTERVAL
done

peak=$(sample VmHWM)
rss_now=$(sample VmRSS)
thr=$(threads)
open_fds=$(fds)
cpu_total=$(cpu_ticks)

median=$(printf '%s\n' "${rss_values[@]}" | sort -n | awk '{ v[NR]=$1 } END { print v[int((NR+1)/2)] }')
first=${rss_values[0]}
last_rss=${rss_values[$((${#rss_values[@]} - 1))]}

printf '| metric | value |\n| --- | --- |\n'
printf '| cold start to steady CPU | %s ms |\n' "${ready_ms:-n/a}"
printf '| RSS after launch | %s MiB |\n' "$(awk -v v=$first 'BEGIN { printf "%.1f", v/1024 }')"
printf '| RSS median (%ss) | %s MiB |\n' "$SECONDS_TO_WATCH" "$(awk -v v=$median 'BEGIN { printf "%.1f", v/1024 }')"
printf '| RSS last | %s MiB |\n' "$(awk -v v=$last_rss 'BEGIN { printf "%.1f", v/1024 }')"
printf '| RSS now | %s MiB |\n' "$(awk -v v=$rss_now 'BEGIN { printf "%.1f", v/1024 }')"
printf '| peak RSS (VmHWM) | %s MiB |\n' "$(awk -v v=$peak 'BEGIN { printf "%.1f", v/1024 }')"
printf '| threads | %s |\n' "$thr"
printf '| open file descriptors | %s |\n' "$open_fds"
printf '| CPU seconds burned | %s |\n' "$(awk -v t=$cpu_total 'BEGIN { printf "%.2f", t/100 }')"
