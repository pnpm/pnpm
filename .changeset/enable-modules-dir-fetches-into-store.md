---
"pacquet": patch
"@pnpm/napi": patch
---

`enable-modules-dir=false` (`enableModulesDir: false` through the Node.js addon) fetches every package into the store again, as pnpm v10 did, while still writing nothing under `node_modules`. The setting exists for a `node_modules` that something else mounts from the store, such as a FUSE daemon, and that consumer no longer has to download each package on first access. A plain `--lockfile-only` run still fetches nothing.
