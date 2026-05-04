---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fixed `pnpm publish` to honor `publishConfig.registry` from `package.json` when publishing a single package. The native publish flow introduced in v11 was reading the registry from `.npmrc` only, ignoring the per-package override [#11419](https://github.com/pnpm/pnpm/issues/11419).
