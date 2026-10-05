---
"@pnpm/cli.parse-cli-args": patch
"@pnpm/config.reader": patch
"pnpm": patch
---

`pnpm config get` and `pnpm config list` with `--global` or `--location=global` now show only the global configuration. Both flags included the project's `.npmrc` before. `--location=global` also included the project's `pnpm-workspace.yaml`. `pnpm config get --global` failed when the global bin directory was not in PATH [#16598](https://github.com/pnpm/pnpm/issues/16598).
