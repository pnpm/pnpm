---
"@pnpm/config.reader": patch
"pnpm": patch
---

Warn when `package.json` contains a legacy `pnpm` field with settings pnpm no longer reads from `package.json` (e.g. `pnpm.overrides`, `pnpm.patchedDependencies`). Previously these were silently ignored after the upgrade from v10, leaving users unaware that their overrides/patched dependencies had stopped taking effect [#11677](https://github.com/pnpm/pnpm/issues/11677).
