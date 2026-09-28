---
"@pnpm/bins.linker": patch
"@pnpm/building.after-install": patch
"pnpm": patch
---

`pnpm rebuild` with `nodeLinker: hoisted` no longer puts one package's parent `node_modules/.bin` directories on the `PATH` of the packages it builds after it.
