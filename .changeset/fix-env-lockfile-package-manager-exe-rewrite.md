---
"@pnpm/installing.env-installer": patch
"pacquet": patch
"pnpm": patch
---

Fixed an issue where non-install commands such as `pnpm list` or `pnpm version` rewrote `packageManagerDependencies` when `@pnpm/exe` was present in the lockfile [#14926](https://github.com/pnpm/pnpm/issues/14926).
