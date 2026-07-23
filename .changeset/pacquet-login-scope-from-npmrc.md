---
"pacquet": patch
---

`pnpm login` / `pnpm adduser` now read the `scope` option from `.npmrc` and `pnpm-workspace.yaml`, not only from the `--scope` command-line flag. When `scope` is configured, the granted token is keyed to that scope and the scope-to-registry mapping is recorded, matching the TypeScript pnpm CLI. `--scope` still takes precedence when both are set.
