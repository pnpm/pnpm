---
"@pnpm/lockfile.to-pnp": patch
"pnpm": patch
---

With `nodeLinker: pnp`, a workspace package can now require another workspace package it depends on. Previously this failed with "Cannot find module", and on Windows the generated `.pnp.cjs` also used backslashes in workspace dependency paths [#3567](https://github.com/pnpm/pnpm/issues/3567).
