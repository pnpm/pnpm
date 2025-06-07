---
"@pnpm/plugin-commands-installation": minor
"@pnpm/plugin-commands-deploy": patch
"pnpm": patch
---

Fix a bug in which `pnpm deploy` fails due to overridden dependencies having peer dependencies causing `ERR_PNPM_OUTDATED_LOCKFILE` [#9595](https://github.com/pnpm/pnpm/issues/9595).
