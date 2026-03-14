---
"@pnpm/config": patch
"@pnpm/core": minor
"@pnpm/headless": minor
"@pnpm/plugin-commands-installation": minor
"pnpm": minor
---

Added a new setting `virtualStoreOnly` that populates the virtual store without creating importer symlinks, hoisting, bin links, or running lifecycle scripts. This is useful for pre-populating a store (e.g., in Nix builds) without creating unnecessary project-level artifacts. `pnpm fetch` now uses this mode internally [#10840](https://github.com/pnpm/pnpm/issues/10840).
