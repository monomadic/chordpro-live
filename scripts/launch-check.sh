#!/bin/sh
# Launches the viewer N times (default 20) and reports, from each run's
# CPL_TIMING log, whether the last accepted page was actually presented.
# A launch where it was not is a window left blank until the next keypress.
#
#   scripts/launch-check.sh [count] [cpl --view arguments...]
#   scripts/launch-check.sh 20 --view page-inline
#
# Pass --rebuild as the first argument to relink the binary before every
# launch: a freshly linked binary launches more slowly, which is when the
# blank window used to appear.
set -u
rebuild=0
[ "${1:-}" = "--rebuild" ] && { rebuild=1; shift; }
count=${1:-20}
[ $# -gt 0 ] && shift
[ $# -eq 0 ] && set -- --view page-inline
cd "$(dirname "$0")/.." || exit 1
cargo build -q || exit 1
log=$(mktemp)
bad=0
retried=0
i=1
while [ "$i" -le "$count" ]; do
  if [ "$rebuild" -eq 1 ]; then touch src/main.rs; cargo build -q || exit 1; fi
  CPL_TIMING=1 ./target/debug/cpl "$@" 2>"$log" &
  pid=$!
  sleep 2
  kill "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
  if ! awk '/^accept/{a=NR} /^present Yes/{y=NR} END{exit !(a && y>a)}' "$log"; then
    bad=$((bad + 1))
    echo "launch $i: last accepted page was never presented"
    cat "$log"
  fi
  if awk '/^accept/{a=1; s=0} /Skipped/{if(a) s=1} /^present Yes/{if(s) r=1; s=0} END{exit !r}' "$log"; then
    retried=$((retried + 1))
  fi
  i=$((i + 1))
done
rm -f "$log"
echo "$count launches: $bad blank, $retried recovered by the present retry"
[ "$bad" -eq 0 ]
