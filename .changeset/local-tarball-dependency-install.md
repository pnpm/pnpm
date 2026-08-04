---
"pacquet": patch
---

Fixed `pnpm install` silently skipping a local `file:*.tgz` dependency: the package is now extracted into the virtual store, recorded under `packages:` and `snapshots:`, and linked into `node_modules` [#13379](https://github.com/pnpm/pnpm/issues/13379).
