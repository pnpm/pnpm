---
"pacquet": patch
---

Fixed detached child processes being terminated on Windows when another program launches `pnpm` directly, without a shell, as `nr` from `@antfu/ni` does [#14447](https://github.com/pnpm/pnpm/issues/14447).
