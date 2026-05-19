#!/bin/bash
set -euo pipefail

# Thin wrapper around `pacquet/tasks/integrated-benchmark`. Builds two
# pnpm revisions (the current branch and `main`) and runs hyperfine for
# each of the six scenarios that used to live in this script.
#
# Scenarios, registry choice, and runner behaviour are preserved exactly
# as before; the orchestration logic is shared with the pacquet bench.
#
# Prerequisites: cargo, hyperfine, pnpm, node, git.
#
# Env vars: WARMUP (default 1), RUNS (default 10).
#
# Usage: ./benchmarks/bench.sh

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURE_DIR="$REPO_ROOT/benchmarks/fixture"
WARMUP="${WARMUP:-1}"
RUNS="${RUNS:-10}"
BENCH_DIR="$(mktemp -d "${TMPDIR:-/tmp}/pnpm-bench.XXXXXX")"

for tool in cargo hyperfine pnpm node git; do
  if ! command -v "$tool" >/dev/null; then
    echo "error: $tool not on PATH" >&2
    exit 1
  fi
done

echo "── Building integrated-benchmark ──"
cargo build --release --bin=integrated-benchmark --manifest-path "$REPO_ROOT/Cargo.toml"
BIN="$REPO_ROOT/target/release/integrated-benchmark"

# Scenario list mirrors bench.sh's original six. Order = the order they
# were measured before; `generate-results.js` used this same order.
SCENARIOS=(
  "frozen-lockfile-hot-cache:Headless (warm store+cache)"
  "peek:Re-resolution (add dep, warm)"
  "full-resolution:Full resolution (warm, no lockfile)"
  "frozen-lockfile:Headless (cold store+cache)"
  "clean-install:Cold install (nothing warm)"
  "gvs-warm:GVS warm reinstall (warm global store)"
)

# Pre-build both revisions once. Subsequent scenario invocations reuse
# the cloned source dirs and per-revision pnpm bundles, so the per-
# scenario build step is a no-op (pnpm install / tsgo --build are
# incremental).
echo "── Pre-building pnpm revisions ──"
"$BIN" \
  --pnpm-repository "$REPO_ROOT" \
  --work-env "$BENCH_DIR/work-env" \
  --build-only \
  pnpm@HEAD pnpm@main

results_md="$BENCH_DIR/results.md"
{
  echo "# Benchmark Results"
  echo
  echo "| # | Scenario | main | HEAD |"
  echo "|---|---|---|---|"
} > "$results_md"

i=1
for entry in "${SCENARIOS[@]}"; do
  scenario="${entry%%:*}"
  label="${entry#*:}"

  echo ""
  echo "━━━ Benchmark $i: $label ━━━"

  "$BIN" \
    --scenario "$scenario" \
    --registry npm \
    --pnpm-repository "$REPO_ROOT" \
    --fixture-dir "$FIXTURE_DIR" \
    --work-env "$BENCH_DIR/work-env" \
    --warmup "$WARMUP" \
    --runs "$RUNS" \
    --ignore-failure \
    pnpm@main pnpm@HEAD

  cp "$BENCH_DIR/work-env/BENCHMARK_REPORT.md"   "$BENCH_DIR/${scenario}.md"
  cp "$BENCH_DIR/work-env/BENCHMARK_REPORT.json" "$BENCH_DIR/${scenario}.json"

  # Pull mean ± stddev for each variant out of the hyperfine JSON for
  # the consolidated table. Falls back to "n/a" if jq isn't on PATH or
  # the file is missing.
  read_cell() {
    local target=$1
    if ! command -v jq >/dev/null; then
      echo "n/a"
      return
    fi
    jq -r --arg t "$target" '
      .results[] | select(.command == $t) |
      "\((.mean*1000|floor)/1000)s ± \((.stddev*1000|floor)/1000)s"
    ' "$BENCH_DIR/${scenario}.json" 2>/dev/null || echo "n/a"
  }

  main_cell=$(read_cell "pnpm@main")
  head_cell=$(read_cell "pnpm@HEAD")
  echo "| $i | $label | ${main_cell:-n/a} | ${head_cell:-n/a} |" >> "$results_md"

  i=$((i + 1))
done

echo
echo "━━━ Results ━━━"
cat "$results_md"
echo
echo "Results saved to: $results_md"
echo "Temp directory kept at: $BENCH_DIR"
echo "Remove with: rm -rf $BENCH_DIR"
