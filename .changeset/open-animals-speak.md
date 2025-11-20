---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

The installation should fail if an optional dependency cannot be installed due to a trust policy check failure [#10208](https://github.com/pnpm/pnpm/issues/10208).
