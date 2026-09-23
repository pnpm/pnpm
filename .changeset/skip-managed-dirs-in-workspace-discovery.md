---
"pacquet": patch
---

pnpm no longer treats manifests inside its store, cache, state, or modules directories as workspace projects. Before, a `storeDir` inside the workspace could let lifecycle scripts of packages in the store run without `allowBuilds` approval [#15033](https://github.com/pnpm/pnpm/issues/15033).
