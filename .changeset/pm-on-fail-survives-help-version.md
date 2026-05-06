---
"@pnpm/cli.parse-cli-args": patch
"pnpm": patch
---

`--pm-on-fail=ignore` (and other universal options like `--loglevel`, `--reporter`) is now honored when combined with `--help` or `--version`. Previously the CLI argument parser short-circuited those flags before universal options were preserved, so `pnpm audit --pm-on-fail=ignore --help` and `pnpm --pm-on-fail=ignore --version` reported the strict packageManager mismatch instead of running the requested action [#11487](https://github.com/pnpm/pnpm/issues/11487).
