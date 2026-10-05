---
"@pnpm/cli.parse-cli-args": patch
"pnpm": patch
---

`pnpm config` now treats `--global` and `--location=global` as the same option. `pnpm config get --location=global` and `pnpm config list --location=global` included settings from the project's `pnpm-workspace.yaml` before. `pnpm config get --global` failed when the global bin directory was not in PATH [#16598](https://github.com/pnpm/pnpm/issues/16598).
