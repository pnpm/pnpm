---
"@pnpm/resolve-dependencies": minor
"@pnpm/headless": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

**Experimental**. Added support for global virtual stores. When the global virtual store is enabled, `node_modules` doesn’t contain regular files, only symlinks to a central virtual store (by default the central store is located at `<store-path>/links`; run `pnpm store path` to find `<store-path>`).

To enable the global virtual store, add `enableGlobalVirtualStore: true` to your root `pnpm-workspace.yaml`.  Change its location with the `globalVirtualStoreDir` setting.

A global virtual store can make installations significantly faster when a warm cache is present. In CI, however, it will probably slow installations because there is usually no cache.

Related PR: [#8190](https://github.com/pnpm/pnpm/pull/8190).
