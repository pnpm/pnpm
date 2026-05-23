# pnpm Benchmarks

Compares `pnpm install` performance between the current branch (`HEAD`) and
`main`, across the six scenarios listed below.

This wrapper builds both pnpm revisions and runs hyperfine through the
shared Rust orchestrator at
[`pacquet/tasks/integrated-benchmark`](../pacquet/tasks/integrated-benchmark/),
so scenario / fixture / workspace / install-script / report generation
stay consistent with the pacquet benchmark.

## Prerequisites

- `cargo` (install Rust via [rustup](https://rustup.rs) if you don't have it).
- `hyperfine`, `pnpm`, `node`, `git` on `$PATH`.

## Usage

```sh
./benchmarks/bench.sh
```

The script:

1. Builds the `integrated-benchmark` binary in release mode.
2. Clones the current repo into the temp work-env once per revision
   (`HEAD` and `main`) and runs `pnpm install && pnpm run compile-only`
   in each to produce `pnpm/dist/pnpm.mjs`. `compile-only` skips the
   `update-manifests` pass that the root `compile` script does — it
   would rewrite tracked files and trigger a second install per
   revision, neither of which the bench needs.
3. Runs hyperfine on each scenario with `--registry=npm` (hits
   `registry.npmjs.org` directly, no proxy — same as before).
4. Writes a per-scenario `BENCHMARK_REPORT.md` / `.json` and a
   consolidated `results.md` into the temp work-env. The path is printed
   at the end of the run.
5. Emits `bencher-results.json` — a hyperfine-shaped file with one
   result per scenario (the `@HEAD` revision only, `command` renamed to
   the scenario name) that the `Benchmarks` GitHub Actions workflow
   uploads to [Bencher](https://bencher.dev) for continuous tracking.

## Scenarios

| # | Name (orchestrator) | Lockfile | Store + Cache | Description |
|---|---|---|---|---|
| 1 | `frozen-lockfile-hot-cache` | ✔ frozen | warm | Headless install (repeat install with warm store) |
| 2 | `peek` | ✔ + add dep | warm | Re-resolution: add a new dep to an existing lockfile |
| 3 | `full-resolution` | ✗ | warm | Resolve everything from scratch with warm cache |
| 4 | `frozen-lockfile` | ✔ frozen | cold | Typical CI install — fetch all packages with lockfile |
| 5 | `clean-install` | ✗ | cold | True cold start — nothing cached |
| 6 | `gvs-warm` | ✔ frozen | warm + GVS | GVS warm reinstall (frozen lockfile, warm global virtual store) |

All scenarios use `--ignore-scripts` and isolated store/cache directories per revision.

## Fixture

The fixture lives at [`fixture/`](./fixture/) — a synthetic
`package.json` with ~80 typical front-end dependencies, plus a committed
`pnpm-lock.yaml` (generated once with `pnpm install --lockfile-only`).
The lockfile is checked in so every CI run starts from the same
resolution graph regardless of registry drift.

## Configuration

Environment variables read by `bench.sh`:

- `WARMUP` — number of warmup runs before timing (default: 1)
- `RUNS` — number of timed runs per benchmark (default: 10)
