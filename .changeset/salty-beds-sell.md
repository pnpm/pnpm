---
"@pnpm/lockfile.settings-checker": patch
"@pnpm/lockfile.verification": patch
"@pnpm/core": patch
"@pnpm/deps.status": patch
---

Properly throw a frozen lockfile error when changing catalogs defined in `pnpm-workspace.yaml` and running `pnpm install --frozen-lockfile`. This previously passed silently as reported in [#9369](https://github.com/pnpm/pnpm/issues/9369).
