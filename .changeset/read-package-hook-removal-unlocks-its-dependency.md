---
"pacquet": patch
---

Removing a `readPackage` hook from a project's pnpmfile now takes the dependencies it added back out of `pnpm-lock.yaml` [#3735](https://github.com/pnpm/pnpm/issues/3735).
