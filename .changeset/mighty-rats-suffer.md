---
"@pnpm/resolve-dependencies": patch
"@pnpm/lockfile-utils": patch
"pnpm": patch
---

Fix `Cannot read properties of undefined (reading 'missingPeersOfChildren')` exception that happens on install [#8041](https://github.com/pnpm/pnpm/issues/8041).
