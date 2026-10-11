---
"pacquet": minor
---

Added the `nodePackageMap` setting, which groups the package-map settings. `nodePackageMap.mode: write-only` writes `node_modules/.package-map.json` for other tools without making Node.js load it. `mode: load` also makes Node.js load the map, like `nodeExperimentalPackageMap: true`. `nodePackageMap.type` replaces `nodePackageMapType`. The old settings still work [pnpm/pnpm#14937](https://github.com/pnpm/pnpm/issues/14937).

```yaml
nodePackageMap:
  mode: write-only
  type: loose
```
