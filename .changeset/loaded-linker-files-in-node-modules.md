---
"pacquet": patch
---

The experimental loaded linker now writes its generated files to `node_modules`, which projects already ignore in git. The store manifest and loader are now `node_modules/.pnpm/.store-manifest.json` and `node_modules/.pnpm/.store-loader.mjs`, and bin shims are in `node_modules/.bin`. The project root no longer gets a `.pnpm` directory, `.pnpm-store.json`, or `.pnpm-store-loader.mjs`. Delete them after reinstalling.
