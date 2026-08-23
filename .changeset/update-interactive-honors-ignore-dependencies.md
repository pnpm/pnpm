---
"pacquet": patch
---

`pnpm outdated` and `pnpm update --interactive` now leave out the dependencies listed in `updateConfig.ignoreDependencies`, instead of reporting them and offering them for update.

`pnpm -r update --latest --depth 0 <selector>` now fails with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` when no project in the workspace declares a matching dependency, instead of silently doing nothing.
