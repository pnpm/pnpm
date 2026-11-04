---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm update --depth 0` without selectors now does nothing when every direct dependency is listed in `update.ignoreDeps`. Before, it failed with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES` [pnpm/tasks#251](https://github.com/pnpm/tasks/issues/251).
