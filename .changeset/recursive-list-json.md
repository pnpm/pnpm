---
"@pnpm/deps.inspection.commands": patch
"@pnpm/deps.inspection.list": patch
"pnpm": patch
"pacquet": patch
---

`pnpm -r list --json` now returns one valid JSON array when `sharedWorkspaceLockfile` is `false`. Recursive listing in all output formats now uses each project's installed package paths, including `packageConfigs.modulesDir`, and includes the package details requested by `--long` [#15011](https://github.com/pnpm/pnpm/issues/15011).
