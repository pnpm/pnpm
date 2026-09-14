---
"pnpm": patch
"pacquet": patch
---

`pnpm --version` no longer fails when the pnpm version a project pins cannot be installed or recorded, as in a sandbox with a read-only filesystem. pnpm reports why and prints the version of the running CLI [#14831](https://github.com/pnpm/pnpm/issues/14831).
