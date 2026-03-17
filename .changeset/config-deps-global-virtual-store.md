---
"@pnpm/config.deps-installer": minor
"pnpm": minor
---

Config dependencies are now installed into the global virtual store (`{storeDir}/links/`) and symlinked into `node_modules/.pnpm-config/`. This allows config dependencies to be shared across projects that use the same store, avoiding redundant fetches and imports.
