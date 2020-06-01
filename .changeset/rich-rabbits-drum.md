---
"@pnpm/plugin-commands-script-runners": minor
---

The `run` and `exec` commands may use the `--parallel` option.

`--parallel` completely disregards concurrency and topological sorting,
running a given script immediately in all matching packages
with prefixed streaming output. This is the preferred flag
for long-running processes such as watch run over many packages.

For example: `pnpm run --parallel watch`
