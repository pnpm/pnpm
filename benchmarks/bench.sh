#!/bin/bash
set -euo pipefail

# Benchmark script for pnpm install performance.
# Compares the current (active) branch against a baseline checkout of main.
#
# Prerequisites:
#   - hyperfine (https://github.com/sharkdp/hyperfine)
#   - The current branch must be compiled (pnpm run compile)
#
# Usage:
#   ./benchmarks/bench.sh [path-to-main-checkout]
#
# If no path is given, a git worktree for main is created automatically,
# dependencies are installed, and pnpm is compiled in it.
#
# Examples:
#   pnpm run compile
#   ./benchmarks/bench.sh
#   ./benchmarks/bench.sh /Volumes/src/pnpm/pnpm/main

BRANCH_DIR="$(cd "$(dirname "$0")/.." && pwd)"

if [ -n "${1:-}" ]; then
  MAIN_DIR="$1"
else
  # Look for an existing worktree that has main checked out
  EXISTING=$(git -C "$BRANCH_DIR" worktree list --porcelain \
    | awk '/^worktree /{wt=$2} /^branch refs\/heads\/main$/{print wt}')

  if [ -n "$EXISTING" ]; then
    MAIN_DIR="$EXISTING"
    echo "── Using existing main worktree at $MAIN_DIR ──"
  else
    MAIN_DIR="$BRANCH_DIR/../.pnpm-bench-main"
    echo "── Creating main worktree at $MAIN_DIR ──"
    git -C "$BRANCH_DIR" worktree add "$MAIN_DIR" main
  fi

  cd "$MAIN_DIR"
  echo "Installing dependencies..."
  pnpm install
  echo "Compiling..."
  pnpm run compile
  echo ""
  cd "$BRANCH_DIR"
fi

BENCH_DIR="$(mktemp -d "${TMPDIR:-/tmp}/pnpm-bench.XXXXXX")"
WARMUP="${WARMUP:-1}"
RUNS="${RUNS:-10}"

# ── Per-variant configuration ─────────────────────────────────────────────

resolve_pnpm_bin() {
  local dir="$1"
  if [ -f "$dir/pnpm/dist/pnpm.mjs" ]; then
    echo "$dir/pnpm/dist/pnpm.mjs"
  else
    echo "$dir/pnpm/dist/pnpm.cjs"
  fi
}

VARIANTS=("main" "branch")
VARIANT_DIRS=("$MAIN_DIR" "$BRANCH_DIR")
VARIANT_BINS=("$(resolve_pnpm_bin "$MAIN_DIR")" "$(resolve_pnpm_bin "$BRANCH_DIR")")
VARIANT_PROJECTS=("$BENCH_DIR/project-main" "$BENCH_DIR/project-branch")
VARIANT_STORES=("$BENCH_DIR/store-main" "$BENCH_DIR/store-branch")
VARIANT_CACHES=("$BENCH_DIR/cache-main" "$BENCH_DIR/cache-branch")

# ── Validation ──────────────────────────────────────────────────────────────

if ! command -v hyperfine &>/dev/null; then
  echo "error: hyperfine is required. Install via: brew install hyperfine" >&2
  exit 1
fi

for bin in "${VARIANT_BINS[@]}"; do
  if [ ! -f "$bin" ]; then
    echo "error: compiled pnpm not found at $bin" >&2
    echo "Run 'pnpm run compile' in both repos first." >&2
    exit 1
  fi
done

for i in "${!VARIANTS[@]}"; do
  # Run --version from BENCH_DIR to avoid pnpm's manage-package-manager-versions
  # switching the CLI based on a packageManager field in the current directory.
  echo "${VARIANTS[$i]}:   $(cd "$BENCH_DIR" && node "${VARIANT_BINS[$i]}" --version)  (${VARIANT_DIRS[$i]})"
done
echo "workdir: $BENCH_DIR"
echo ""

# ── Project setup ───────────────────────────────────────────────────────────
# Each variant gets its own project directory with isolated store and cache
# so there is no shared state between them.

for i in "${!VARIANTS[@]}"; do
  dir="${VARIANT_PROJECTS[$i]}"
  mkdir -p "$dir" "${VARIANT_CACHES[$i]}"
  cp "$BRANCH_DIR/benchmarks/fixture.package.json" "$dir/package.json"
  printf "storeDir: %s\ncacheDir: %s\n" "${VARIANT_STORES[$i]}" "${VARIANT_CACHES[$i]}" > "$dir/pnpm-workspace.yaml"
done

# Keep a pristine copy of package.json for the peek benchmark
cp "$BRANCH_DIR/benchmarks/fixture.package.json" "$BENCH_DIR/original-package.json"

# ── Populate stores and caches ─────────────────────────────────────────────
# A full install populates both the content-addressable store and the
# registry metadata cache for each variant.

for i in "${!VARIANTS[@]}"; do
  label="${VARIANTS[$i]}"
  dir="${VARIANT_PROJECTS[$i]}"
  bin="${VARIANT_BINS[$i]}"
  echo "Populating store and cache for $label..."
  rm -rf "$dir/node_modules" "$dir/pnpm-lock.yaml"
  cd "$dir" && node "$bin" install --ignore-scripts --no-frozen-lockfile >/dev/null 2>&1
  if [ ! -f "$dir/pnpm-lock.yaml" ]; then
    echo "error: pnpm-lock.yaml was not created for $label in $dir" >&2
    exit 1
  fi
  cp "$dir/pnpm-lock.yaml" "$BENCH_DIR/saved-lockfile-$label.yaml"
done

# ── Helper ──────────────────────────────────────────────────────────────────
# run_bench <name> <prepare_template> <cmd_template>
#
# Templates use placeholders that are substituted per variant:
#   {project}  → project directory
#   {bin}      → compiled pnpm binary
#   {store}    → store directory
#   {cache}    → cache directory
#   {lockfile} → saved lockfile path

run_bench() {
  local bench_name=$1
  local prepare_tpl=$2
  local cmd_tpl=$3

  for i in "${!VARIANTS[@]}"; do
    local variant="${VARIANTS[$i]}"
    local project="${VARIANT_PROJECTS[$i]}"
    local bin="${VARIANT_BINS[$i]}"
    local store="${VARIANT_STORES[$i]}"
    local cache="${VARIANT_CACHES[$i]}"
    local lockfile="$BENCH_DIR/saved-lockfile-$variant.yaml"

    local prepare="$prepare_tpl"
    prepare="${prepare//\{project\}/$project}"
    prepare="${prepare//\{bin\}/$bin}"
    prepare="${prepare//\{store\}/$store}"
    prepare="${prepare//\{cache\}/$cache}"
    prepare="${prepare//\{lockfile\}/$lockfile}"

    local cmd="$cmd_tpl"
    cmd="${cmd//\{project\}/$project}"
    cmd="${cmd//\{bin\}/$bin}"
    cmd="${cmd//\{store\}/$store}"
    cmd="${cmd//\{cache\}/$cache}"
    cmd="${cmd//\{lockfile\}/$lockfile}"

    echo ""
    echo "  $variant:"
    hyperfine \
      --warmup "$WARMUP" \
      --runs "$RUNS" \
      --ignore-failure \
      --prepare "$prepare" \
      --command-name "$variant" \
      "$cmd" \
      --export-json "$BENCH_DIR/${bench_name}-${variant}.json" \
      || true
  done
}

# ── Benchmark 1: Headless install ──────────────────────────────────────────
# Lockfile present, node_modules deleted, store and cache warm.
# This is the common "CI install" or "fresh clone + install" path.

echo ""
echo "━━━ Benchmark 1: Headless install (frozen lockfile, warm store+cache) ━━━"

run_bench "headless" \
  "rm -rf {project}/node_modules && cp {lockfile} {project}/pnpm-lock.yaml" \
  "cd {project} && node {bin} install --frozen-lockfile --ignore-scripts >/dev/null 2>&1"

# ── Benchmark 2: Re-resolution with existing lockfile ─────────────────────
# Lockfile present, add a new dependency to trigger re-resolution.
# Store and cache warm. This exercises the peekManifestFromStore path.

echo ""
echo "━━━ Benchmark 2: Re-resolution (add dep to existing lockfile, warm store+cache) ━━━"

run_bench "peek" \
  "rm -rf {project}/node_modules && cp {lockfile} {project}/pnpm-lock.yaml && cp $BENCH_DIR/original-package.json {project}/package.json" \
  "cd {project} && node {bin} add is-odd --ignore-scripts >/dev/null 2>&1"

# ── Benchmark 3: Full resolution (warm store+cache) ──────────────────────
# No lockfile, no node_modules, store and cache warm.
# Resolution runs for all packages using cached registry metadata.

echo ""
echo "━━━ Benchmark 3: Full resolution (no lockfile, warm store+cache) ━━━"

run_bench "nolockfile" \
  "rm -rf {project}/node_modules {project}/pnpm-lock.yaml && cp $BENCH_DIR/original-package.json {project}/package.json" \
  "cd {project} && node {bin} install --ignore-scripts --no-frozen-lockfile >/dev/null 2>&1"

# ── Benchmark 4: Headless cold (lockfile, no store, no cache) ─────────────
# Lockfile present, but store and cache are empty.
# This tests the fetch-from-registry + link path guided by a lockfile.

echo ""
echo "━━━ Benchmark 4: Headless install (frozen lockfile, cold store+cache) ━━━"

run_bench "headless-cold" \
  "rm -rf {project}/node_modules {store} {cache} && cp {lockfile} {project}/pnpm-lock.yaml" \
  "cd {project} && node {bin} install --frozen-lockfile --ignore-scripts >/dev/null 2>&1"

# ── Benchmark 5: Cold install (no store, no cache, no lockfile) ───────────
# Everything is deleted before each run. This is the true cold start.

echo ""
echo "━━━ Benchmark 5: Cold install (no store, no cache, no lockfile) ━━━"

run_bench "cold" \
  "rm -rf {project}/node_modules {project}/pnpm-lock.yaml {store} {cache} && cp $BENCH_DIR/original-package.json {project}/package.json" \
  "cd {project} && node {bin} install --ignore-scripts --no-frozen-lockfile >/dev/null 2>&1"

# ── Summary ─────────────────────────────────────────────────────────────────

RESULTS_MD="$BENCH_DIR/results.md"

echo ""
echo "━━━ Results ━━━"
node "$BRANCH_DIR/benchmarks/generate-results.js" "$BENCH_DIR" "$RESULTS_MD"
echo ""
echo "Results saved to: $RESULTS_MD"

# Cleanup
for project in "${VARIANT_PROJECTS[@]}"; do
  rm -rf "$project/node_modules"
done
echo ""
echo "Temp directory kept at: $BENCH_DIR"
echo "Remove with: rm -rf $BENCH_DIR"
