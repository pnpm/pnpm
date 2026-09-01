---
"pacquet": patch
---

`minimumReleaseAgeStrict` now defaults to `true` when `minimumReleaseAge` is explicitly configured — through `pnpm-workspace.yaml`, the global `config.yaml`, a `PNPM_CONFIG_*` variable, or a CLI flag — as documented. Previously an explicit cutoff was treated as non-strict, so immature versions were silently added to `minimumReleaseAgeExclude` instead of being gated with a prompt [#14409](https://github.com/pnpm/pnpm/issues/14409). The built-in 1440-minute default stays non-strict.
