---
"@pnpm/cli.parse-cli-args": patch
"pnpm": patch
---

`pnpm config get --location=global` and `pnpm config list --location=global` now ignore the project's `pnpm-workspace.yaml`, as `--global` already did. They included its settings before [#16598](https://github.com/pnpm/pnpm/issues/16598).
