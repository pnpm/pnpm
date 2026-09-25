---
"@pnpm/releasing.commands": patch
"pacquet": patch
"pnpm": patch
---

Fixed `pnpm deploy --prod` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when the deployed project declares a `devEngines.runtime` with `onFail: download`. The runtime stays out of the deployed `node_modules` with the rest of the dev dependencies [#15703](https://github.com/pnpm/pnpm/issues/15703).
