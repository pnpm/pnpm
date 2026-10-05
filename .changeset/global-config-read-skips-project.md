---
"pacquet": patch
---

`pnpm config get --global` and `pnpm config list --global` now show only the global configuration, also when run inside a project. Settings from the project's `pnpm-workspace.yaml` and `.npmrc` were included before. The same applies to `--location=global` [#16598](https://github.com/pnpm/pnpm/issues/16598).
