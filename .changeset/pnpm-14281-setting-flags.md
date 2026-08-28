---
"pacquet": patch
---

The settings that pnpm accepts as command-line flags are recognized again: `--package-import-method`, `--hoist-pattern`, `--public-hoist-pattern`, `--no-hoist`, `--global-dir`, `--virtual-store-dir`, `--modules-dir`, `--child-concurrency`, `--no-lockfile`, `--strict-peer-dependencies`, `--side-effects-cache`, `--side-effects-cache-readonly`, `--trust-policy`, `--trust-policy-exclude`, `--trust-policy-ignore-after`, and `--optimistic-repeat-install`. Each is accepted anywhere on the command line, spelled either `--setting=value` or `--setting value`, and overrides the same setting read from `pnpm-workspace.yaml` or `.npmrc` [#14281](https://github.com/pnpm/pnpm/issues/14281).
