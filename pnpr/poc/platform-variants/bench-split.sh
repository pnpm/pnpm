#!/usr/bin/env bash
# Benchmark whole packuments, packuments split by moving only each version's
# dist, and packuments split into an external-versions map and release
# records, through the CDN-like shaping proxy. Both split forms keep the
# likely picks whole. Each run starts with an empty pnpm cache and store.
# usage: bench-split.sh <runs> <work-dir> <pnpr-config>
# Expects release builds in target/release, and a pnpr config whose upstream
# proxies npmjs (set its maxAge long so the runs do not revalidate).
set -euo pipefail
runs=$1 WORK=$(realpath "$2") CONFIG=$(realpath "$3")
HERE=$(dirname "$(realpath "$0")")
BIN=$HERE/../../../target/release
OUT=$WORK/results.jsonl
P=http://127.0.0.1:7700
: > $OUT

node $HERE/shaper.mjs 7700 7690 &
shaper_pid=$!
pnpr_pid=
trap 'kill $shaper_pid $pnpr_pid 2>/dev/null || true' EXIT
sleep 0.5

one_run() { # <label> <cmd> <profile>
  local dir
  dir=$(mktemp -d -p $WORK runs.XXXX)
  cp $HERE/bench-app.json $dir/package.json
  echo "registry=$P/npm/" > $dir/.npmrc
  # Declared as pnpr, the registry's abbreviated documents serve optional
  # dependencies too; otherwise pnpm reads their full documents for `libc`.
  printf 'storeDir: %s/store\ncacheDir: %s/cache\nminimumReleaseAge: 0\nregistries:\n  %s/npm/: {serverType: pnpr}\n' "$dir" "$dir" "$P" > $dir/pnpm-workspace.yaml
  local args=(install --lockfile-only)
  [[ "$2" == *install ]] && args=(install --ignore-scripts)
  curl -s $P/__stats > /dev/null
  local start end
  start=$(date +%s.%N)
  (cd $dir && $BIN/pnpm "${args[@]}" > $dir/pnpm.log 2>&1) || { echo "pnpm failed in $dir"; tail -20 $dir/pnpm.log; exit 1; }
  end=$(date +%s.%N)
  local pkgs
  pkgs=$(awk '/^packages:/{p=1;next} /^[a-z]/{p=0} p && /^  [^ ].*:$/{n++} END{print n+0}' $dir/pnpm-lock.yaml)
  [ -f $WORK/lock-$1.yaml ] || cp $dir/pnpm-lock.yaml $WORK/lock-$1.yaml
  echo "{\"label\":\"$1\",\"cmd\":\"$2\",\"profile\":\"$3\",\"secs\":$(printf %.3f "$(echo "$end - $start" | bc)"),\"packages\":$pkgs,\"stats\":$(curl -s $P/__stats)}" | tee -a $OUT
  rm -rf $dir
}

for mode in whole split-dist split-map; do
  case $mode in
    whole) threshold=1000000000000000 dist_only=0 ;;
    split-dist) threshold=65536 dist_only=1 ;;
    split-map) threshold=65536 dist_only=0 ;;
  esac
  cd $(dirname $CONFIG)
  PNPR_SPLIT_METADATA_THRESHOLD=$threshold PNPR_SPLIT_DIST_ONLY=$dist_only \
    $BIN/pnpr -c $CONFIG --listen 127.0.0.1:7690 --public-url http://127.0.0.1:7700 > $WORK/pnpr-$mode.log 2>&1 &
  pnpr_pid=$!
  for _ in $(seq 100); do curl -sf http://127.0.0.1:7690/npm/is-number > /dev/null && break; sleep 0.1; done
  curl -s "$P/__flush" > /dev/null
  curl -s "$P/__config?rtt=0&mbit=0" > /dev/null
  one_run $mode warm-cdn none
  one_run $mode warm-cdn-install none
  for profile in fast:10:200 typical:40:50 slow:100:10; do
    IFS=: read -r name rtt mbit <<< "$profile"
    curl -s "$P/__config?rtt=$rtt&mbit=$mbit" > /dev/null
    for _ in $(seq "$runs"); do one_run $mode resolve $name; done
    [ "$name" = typical ] && for _ in $(seq "$runs"); do one_run $mode install $name; done
  done
  kill $pnpr_pid; wait $pnpr_pid 2>/dev/null || true
  pnpr_pid=
done
