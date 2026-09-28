# Repeated version selection

Cache successful normalized deprecation probes for each immutable version slot.
This follows [upm's selective metadata approach](https://github.com/unjs/upm/blob/16ad722f616e5ddb7b55f67440d75b4d1a1c2c09/src/pick.ts):
avoid repeating metadata work during resolution. The Rust cache is a separate
implementation, not a port of an upm cache.

## Measurement

These are resolver-stage benchmarks, not whole-install timings. The example calls
`pick_package_from_meta` against 1,000 releases across 32 ranges, with the newest
99 releases deprecated. Every pick must return `1.0.900`. The `-age` modes use the
cached minimum-release-age view. File-backed modes use a temporary local mirror.
Metadata construction and process startup are outside the timer.

Base: `15a5da5864020375be4c46c905807364f0e941b2`. Candidate: this patch. Twelve alternating
paired process runs on macOS 26.7, arm64, 14 logical CPUs; Rust 1.97.0, the
repository release profile (opt-level 3, fat LTO, one codegen unit). No benchmark
or compilation jobs from this task ran concurrently. Other machine load was not
controlled; post-run load averages were 7.59 / 6.95 / 5.18. Binary SHA-256 hashes
and all samples are in [results.json](results.json).

| Metadata | Picks | Base median | Candidate median | Reduction |
| --- | ---: | ---: | ---: | ---: |
| raw | 1 | 0.500 ms | 0.497 ms | 0.5% |
| raw | 100 | 44.362 ms | 15.188 ms | 65.8% |
| file | 1 | 0.907 ms | 0.923 ms | -1.8% |
| file | 100 | 87.228 ms | 15.547 ms | 82.2% |
| raw-age | 1 | 0.539 ms | 0.549 ms | -2.0% |
| raw-age | 100 | 44.219 ms | 14.949 ms | 66.2% |
| file-age | 1 | 0.997 ms | 1.005 ms | -0.8% |
| file-age | 100 | 87.282 ms | 15.459 ms | 82.3% |

The repeated raw batch ranges were 44.18–46.44 ms before and 15.05–15.45 ms
after. File-backed repeated batches were 86.38–88.74 ms before and
15.36–16.07 ms after. Single-pick timings overlap; no single-pick speedup is
claimed. The change adds one `OnceLock<bool>` per version slot. It leaves mirror
read failures retryable and gives later full hydration precedence over the probe.

A filtered copy receives only already-populated probe results; copies created
before probing do not share subsequent results. The separate `deprecation_probes`
example can measure this control: use `raw-filtered` or `file-filtered` with 100
passes. In a separate 12-pair run, fresh filtered scans were 5.4% and 4.4% slower
respectively. Single fragment scans were 4.8% and 1.8% slower. These controls
show the cache's cost when probes cannot be reused. See [probe-controls.json](probe-controls.json).

## Reproduce

Use independent checkouts and **separate Cargo target directories** for baseline
and candidate. Copy `crates/resolving-npm-resolver/examples/repeated_range_picks.rs`
from the candidate's `pnpm/` directory into the same location in the baseline.
Use the repository dependency setup instructions in each checkout, then run:

```sh
# In the baseline checkout:
CARGO_TARGET_DIR=/tmp/pnpm-base-target cargo build --locked --release \
  -p pnpm-resolving-npm-resolver --example repeated_range_picks
# In the candidate checkout:
CARGO_TARGET_DIR=/tmp/pnpm-candidate-target cargo build --locked --release \
  -p pnpm-resolving-npm-resolver --example repeated_range_picks
```

The executables are in each target directory under
`release/examples/repeated_range_picks`. From the candidate root:

```sh
python3 pnpm/benchmarks/deprecation-probes/compare.py \
  --base /tmp/pnpm-base-target/release/examples/repeated_range_picks \
  --candidate /tmp/pnpm-candidate-target/release/examples/repeated_range_picks
```

The recorded run used a shared Cargo target directory, with each registry source
explicitly invalidated and rebuilt before copying its executable. Build logs
confirmed the corresponding checkout. An earlier run that reused workspace
artifacts was discarded; separate target directories avoid that hazard.
