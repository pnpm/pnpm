---
"@pnpm/lockfile.to-pnp": patch
"pnpm": patch
---

With `nodeLinker: pnp`, a workspace package can now require another workspace package it depends on [#3567](https://github.com/pnpm/pnpm/issues/3567). On Windows, workspace dependency paths in the generated `.pnp.cjs` now use forward slashes.
