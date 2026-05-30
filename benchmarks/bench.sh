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

# Ensure `pnpm@main` resolves locally. `actions/checkout` only creates a
# local ref for the branch it checked out; on a workflow_dispatch run from
# a non-main branch (or after the optional PR-head checkout in
# `benchmark.yml`) there's no `refs/heads/main` for `git rev-parse` to
# hit. Skip the fetch entirely when the local ref already exists, and
# let the fetch surface its real error if it fails.
if ! git -C "$REPO_ROOT" rev-parse --verify --quiet refs/heads/main >/dev/null; then
  echo "── Fetching main into local ref ──"
  git -C "$REPO_ROOT" fetch --no-tags origin main:main
fi

# Scenario list: `slug:Display label`. The slug matches the
# orchestrator's `--scenario` value (the clap-derived kebab-case name
# from `BenchmarkScenario`). All six start with `node_modules` wiped
# — "Fresh" names that target state. "Isolated linker" names the
# `nodeLinker` mode; alternatives (`hoisted`, `pnp`) and populated-
# node_modules counterparts are reserved for future scenarios.
SCENARIOS=(
  "isolated-linker.fresh-restore.hot-cache.hot-store:Isolated linker: fresh restore, hot cache + hot store"
  "isolated-linker.fresh-add-dep.hot-cache.hot-store:Isolated linker: fresh add new dep, hot cache + hot store"
  "isolated-linker.fresh-install.hot-cache.hot-store:Isolated linker: fresh install, hot cache + hot store"
  "isolated-linker.fresh-restore.cold-cache.cold-store:Isolated linker: fresh restore, cold cache + cold store"
  "isolated-linker.fresh-install.cold-cache.cold-store:Isolated linker: fresh install, cold cache + cold store"
  "gvs-linker.fresh-restore.hot-cache.hot-store:GVS linker: fresh restore, hot cache + hot store"
)

# Pre-build both revisions once. Subsequent scenario invocations still
# re-enter the orchestrator's build step (sync_bench_repo + pnpm install
# + pnpm run compile), but `pnpm install` is a no-op on the populated
# node_modules and `tsgo --build` is incremental. `pnpm run bundle`
# (which produces pnpm/dist/pnpm.mjs) does run each time and is not
# incremental — accepted overhead in exchange for keeping the build
# path in one consistent place across pacquet and pnpm benches.
echo "── Pre-building pnpm revisions ──"
"$BIN" \
  --pnpm-repository "$REPO_ROOT" \
  --work-env "$BENCH_DIR/work-env" \
  --build-only \
  pnpm@HEAD pnpm@main

# Pull mean ± stddev for each variant out of a hyperfine JSON into one
# table cell. Falls back to "n/a" if jq isn't on PATH, the file is
# missing, or the target isn't present in the JSON.
read_cell() {
  local target=$1
  local json=$2
  if ! command -v jq >/dev/null; then
    echo "n/a"
    return
  fi
  jq -r --arg t "$target" '
    [.results[] | select(.command == $t)
      | "\((.mean*1000|round)/1000)s ± \((.stddev*1000|round)/1000)s"]
    | first // "n/a"
  ' "$json" 2>/dev/null || echo "n/a"
}

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

  main_cell=$(read_cell "pnpm@main" "$BENCH_DIR/${scenario}.json")
  head_cell=$(read_cell "pnpm@HEAD" "$BENCH_DIR/${scenario}.json")
  echo "| $i | $label | $main_cell | $head_cell |" >> "$results_md"

  i=$((i + 1))
done

# Combine the per-scenario hyperfine JSONs into one Bencher-shaped
# report. Keep only the @HEAD result from each scenario and rename
# `.command` to the scenario name so Bencher's shell_hyperfine adapter
# names the benchmark after the scenario instead of `pnpm@HEAD`.
if command -v jq >/dev/null; then
  bencher_inputs=()
  for entry in "${SCENARIOS[@]}"; do
    scenario="${entry%%:*}"
    jq --arg s "$scenario" \
      '.results |= [.[] | select(.command == "pnpm@HEAD") | .command = $s]' \
      "$BENCH_DIR/${scenario}.json" > "$BENCH_DIR/${scenario}-bencher.json"
    bencher_inputs+=("$BENCH_DIR/${scenario}-bencher.json")
  done
  jq -s '{results: map(.results) | add}' \
    "${bencher_inputs[@]}" > "$BENCH_DIR/bencher-results.json"
else
  echo "warning: jq not on PATH; skipping bencher-results.json generation" >&2
fi

echo
echo "━━━ Results ━━━"
cat "$results_md"
echo
echo "Results saved to: $results_md"
echo "Temp directory kept at: $BENCH_DIR"
echo "Remove with: rm -rf $BENCH_DIR"
