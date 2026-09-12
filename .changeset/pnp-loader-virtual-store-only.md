---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
"pnpm": patch
---

`pnpm fetch`, and any install run with `virtualStoreOnly`, no longer writes a `.pnp.cjs` loader under `nodeLinker: pnp`. These installs populate the virtual store without linking the project, so the loader would have claimed the project resolves out of a store it was never linked into. The importer links and `node_modules/.package-map.json` were already skipped; the PnP loader now follows the same rule.
