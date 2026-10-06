---
"pacquet": patch
---

The experimental loaded linker now writes its generated files to `node_modules`, which projects already ignore in git. The store manifest and loader are `node_modules/.pnpm/.store-manifest.json` and `node_modules/.pnpm/.store-loader.mjs`. Bin shims are in `node_modules/.bin`.

Previously, the loaded linker wrote `.pnpm-store.json` and `.pnpm-store-loader.mjs` to the project root, and a `.pnpm` directory to the root and to each workspace package. Delete them after reinstalling.
