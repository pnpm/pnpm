---
"pacquet": patch
---

With `autoInstallPeers: false`, a package's own optional peer dependencies are no longer added to its importer entry in `pnpm-lock.yaml` (and no longer linked into its `node_modules`) when another workspace project happens to resolve a matching version [#13325](https://github.com/pnpm/pnpm/issues/13325).
