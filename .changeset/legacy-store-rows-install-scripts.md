---
"@pnpm/worker": patch
"pnpm": patch
"pacquet": patch
---

pnpm now finds the install scripts of packages stored by older pnpm versions that recorded neither the build flag nor the manifest in the store index. Before, those scripts could be skipped and the package could be hardlinked to the store.
