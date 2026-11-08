---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when a workspace package linked through a plain semver range, such as `^7.9.5`, has changed to a version outside that range [#17806](https://github.com/pnpm/pnpm/issues/17806).
