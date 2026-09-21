# Integrated benchmark regression checks

Every PR measures HEAD and main on the same runner for each scenario. The PR
check compares those samples. Absolute minimum timings still go to Bencher
for historical tracking, but historical alerts do not fail PRs: the historical
baseline may come from a faster machine.

Run the comparison against a downloaded `integrated-benchmark-report-ubuntu-latest`
artifact:

```sh
node .github/scripts/compare-integrated-benchmarks.mjs /path/to/artifact
node --test .github/scripts/compare-integrated-benchmarks.test.mjs
```

The check requires at least nine successful samples per target. It fails for
missing or invalid data. It checks both pacquet and pnpr, except for the two
client-only resolver scenarios. Adding a scenario requires updating the list
in the comparison script as well as the workflow.

A regression fails the check when the median increases by more than 20% and
the fastest retained HEAD sample exceeds the slowest retained main sample by
more than 20%. The comparison discards `floor(sample count / 10)` samples from
each tail to avoid letting one unusually fast or slow sample decide the result.
The 20% tolerance preserves the historical gate's practical slowdown threshold;
the distribution check adds a conservative noise guard. This is not a statistical
confidence interval. Smaller regressions and noisy slowdowns may pass the gate.

A median slowdown above 20% without that separation is reported as inconclusive,
not a confirmed regression. All timings remain available for performance review.
The gate cannot remove drift during a run: hyperfine measures targets sequentially.

The privileged comment workflow executes the comparison script from the default
branch, treats the PR artifact only as data, and posts the report before enforcing
the result. It publishes the result on the triggering PR's head SHA, retaining
the existing `Bencher Report (pnpm's project)` check name for branch protection.
Bencher uploads omit GitHub integration so historical alerts cannot overwrite
that check. Non-main manual dispatches use the same comparison in their report job.
The existing peer-heavy Rust-versus-TypeScript speedup assertion remains active.
