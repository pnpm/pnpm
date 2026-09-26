---
"pacquet": minor
---

Add the `writePackageMap` setting to write `node_modules/.package-map.json` without enabling Node's experimental package-map resolver. Setting `writePackageMap: true` in `pnpm-workspace.yaml` makes the dependency graph available to other tools while leaving script resolution unchanged [pnpm/pnpm#14937](https://github.com/pnpm/pnpm/issues/14937).
