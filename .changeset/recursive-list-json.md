---
"@pnpm/deps.inspection.commands": patch
"@pnpm/deps.inspection.list": patch
"pnpm": patch
"pacquet": patch
---

`pnpm -r list --json` now returns one valid JSON array when `sharedWorkspaceLockfile` is `false`. In pnpm v12, this output also uses each project's installed package paths and includes the package details requested by `--long` [#15011](https://github.com/pnpm/pnpm/issues/15011).
