---
"pacquet": patch
---

`pnpm install` and `pnpm update` now resolve a dependency range to the newest matching version that is not deprecated. A version already recorded in the lockfile is still used [#15128](https://github.com/pnpm/pnpm/issues/15128).
