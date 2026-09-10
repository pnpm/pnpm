---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
"pnpm": patch
---

pnpm now writes `node_modules/.package-map.json` only when `nodeExperimentalPackageMap` is enabled. Nothing reads the file without that setting. An install that stops writing the map removes the one a previous install left.
