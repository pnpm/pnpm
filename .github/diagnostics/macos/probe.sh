#!/usr/bin/env bash
# Measures two macOS costs in the Rust test job: Gatekeeper scans of
# executables the tests launch, and fcntl(F_FULLFSYNC) flushes.
#
#   probe.sh system              runner, disk and Gatekeeper settings
#   probe.sh flush               F_FULLFSYNC vs fsync latency
#   probe.sh scans SINCE LABEL   Gatekeeper scans logged since SINCE
#   probe.sh exec                first and second launch of fresh pnpm copies
#   probe.sh subset              one test subset with F_FULLFSYNC real, no-op, real
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="${RUNNER_TEMP:-/tmp}/macos-diagnostics"
mkdir -p "$out"

stamp() { date '+%Y-%m-%d %H:%M:%S'; }

show_log() {
  sudo -n /usr/bin/log show "$@" 2>/dev/null || /usr/bin/log show "$@"
}

system() {
  sw_vers
  sysctl -n machdep.cpu.brand_string hw.model hw.ncpu hw.memsize
  echo "Gatekeeper: $(spctl --status 2>&1)"
  echo "SIP: $(csrutil status 2>&1)"
  echo "Developer tools security: $(DevToolsSecurity -status 2>&1)"
  if sudo -n true 2>/dev/null; then echo "passwordless sudo: yes"; else echo "passwordless sudo: no"; fi
  echo "TMPDIR=${TMPDIR:-unset}"
  df -h "${TMPDIR:-/tmp}" "$PWD"
  diskutil info "$(df "${TMPDIR:-/tmp}" | awk 'NR==2 {print $1}')" 2>/dev/null |
    grep -E 'File System Personality|Solid State|Protocol|Device Location|Virtual' || true
}

flush() {
  python3 "$here/flush-probe.py"
}

# Every subcommand reports and returns 0: a probe that finds nothing is a
# result, and the workflow step runs with `bash -e`.
scans() {
  local since="$1" label="$2" file="$out/syspolicy-$2.log"
  show_log --start "$since" --style compact \
    --predicate 'subsystem == "com.apple.syspolicy.exec"' >"$file" || true
  echo "$label: $(grep -c 'evaluateScanResult' "$file") Gatekeeper scans since $since" \
    "($(wc -l <"$file" | tr -d ' ') syspolicy.exec log lines)"
  { grep 'evaluateScanResult' "$file" || true; } | sed -nE 's/.*\(id: ([^)]*)\).*/\1/p' |
    sort | uniq -c | sort -rn | head -10
}

time_exec() {
  local label="$1"
  shift
  /usr/bin/time -p "$@" >/dev/null 2>"$out/time.txt"
  echo "$label: $(awk '/^real/ {print $2}' "$out/time.txt") s"
}

exec_probe() {
  local since
  since="$(stamp)"
  time_exec "target/debug/pnpm" target/debug/pnpm --version
  for copy in 1 2; do
    cp target/debug/pnpm "$out/pnpm-copy-$copy"
    time_exec "fresh copy $copy, first launch" "$out/pnpm-copy-$copy" --version
    time_exec "fresh copy $copy, second launch" "$out/pnpm-copy-$copy" --version
    rm -f "$out/pnpm-copy-$copy"
  done
  scans "$since" exec
}

find_suite() {
  local binary newest=""
  # Object and dep-info files share the prefix; only the executables count.
  for binary in target/debug/deps/suite-*; do
    [[ -x "$binary" && "$(basename "$binary")" != *.* ]] || continue
    grep -qx 'import::import_from_yarn_lock: test' <("$binary" --list 2>/dev/null) || continue
    if [[ -z "$newest" || "$binary" -nt "$newest" ]]; then newest="$binary"; fi
  done
  if [[ -n "$newest" ]]; then echo "$PWD/$newest"; fi
}

# Runs the subset the way `pnpm/scripts/run-rust-tests.mjs` prepares the
# environment, but through the libtest binary so the preloaded library
# reaches every process the tests start.
run_subset() {
  local label="$1" mode="$2" suite="$3" threads="$4" since
  local fsync_log="$out/fullfsync-$label.log" config
  config="$(mktemp -d)"
  : >"$config/npmrc"
  : >"$fsync_log"
  since="$(stamp)"
  (
    while IFS='=' read -r name _; do
      case "$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]')" in
        npm_config_* | pnpm_config_*) unset "$name" ;;
      esac
    done < <(env)
    unset PNPM_HOME XDG_DATA_HOME
    cd pnpm/crates/cli || exit 1
    export PNPM_CONFIG_CI=false PNPM_CONFIG_NPMRC_AUTH_FILE="$config/npmrc" \
      PNPM_TEST_NPMRC_AUTH_FILE="$config/npmrc" XDG_CONFIG_HOME="$config" \
      CARGO_BIN_EXE_pnpm="$OLDPWD/target/debug/pnpm" \
      DYLD_INSERT_LIBRARIES="$out/libfullfsync.dylib" \
      FULLFSYNC_LOG="$fsync_log" FULLFSYNC_MODE="$mode"
    # Launched directly: a SIP-protected wrapper such as /usr/bin/time would
    # drop DYLD_INSERT_LIBRARIES before the suite starts.
    "$suite" --test-threads="$threads" \
      import:: init:: update:: package_manager_check:: update_notifier:: \
      >"$out/subset-$label.txt" 2>&1
  )
  echo "== $label (F_FULLFSYNC $mode, $threads threads)"
  grep -E '^test result:' "$out/subset-$label.txt" || tail -5 "$out/subset-$label.txt"
  awk '{n++; ms+=$2} END {printf "F_FULLFSYNC calls: %d, time inside them: %.1f s\n", n, ms/1000}' "$fsync_log"
  scans "$since" "subset-$label"
  rm -rf "$config"
}

subset() {
  clang -dynamiclib -O2 -o "$out/libfullfsync.dylib" "$here/fullfsync.c" || return
  local suite threads
  suite="$(find_suite)"
  if [[ -z "$suite" ]]; then
    echo "no pnpm-cli suite binary found"
    return
  fi
  threads="$(sysctl -n hw.ncpu)"
  run_subset real-1 real "$suite" "$threads"
  run_subset noop noop "$suite" "$threads"
  run_subset real-2 real "$suite" "$threads"
}

case "${1:-}" in
  system) system ;;
  flush) flush ;;
  scans) scans "$2" "$3" ;;
  exec) exec_probe ;;
  subset) subset ;;
  *)
    echo "usage: $0 system|flush|scans SINCE LABEL|exec|subset" >&2
    exit 2
    ;;
esac
exit 0
