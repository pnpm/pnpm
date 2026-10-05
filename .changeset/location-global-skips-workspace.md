---
"@pnpm/cli.parse-cli-args": patch
"pnpm": patch
---

`pnpm config get --location=global` and `pnpm config list --location=global` no longer show settings from the project's `pnpm-workspace.yaml`, the same as `--global` [#16598](https://github.com/pnpm/pnpm/issues/16598).
