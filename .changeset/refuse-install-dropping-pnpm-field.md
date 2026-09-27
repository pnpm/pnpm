---
"@pnpm/config.reader": patch
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm run` and `pnpm exec` no longer install dependencies automatically when the root `package.json` still keeps `overrides`, `packageExtensions`, `patchedDependencies`, or `ignoredOptionalDependencies` in its `pnpm` field. pnpm no longer reads that field, so the install rewrote the lockfile without those settings. The command now fails and asks to move the settings to `pnpm-workspace.yaml` [#16278](https://github.com/pnpm/pnpm/issues/16278).
