---
"@pnpm/lockfile-file": patch
"pnpm": patch
---

Installation should not fail when there is a local dependency that starts in a directory that starts with the `@` char [#6332](https://github.com/pnpm/pnpm/issues/6332).
