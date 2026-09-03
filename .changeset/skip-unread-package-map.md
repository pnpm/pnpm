---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
---

`node_modules/.package-map.json` is no longer written unless `nodeExperimentalPackageMap` is enabled. Nothing reads the file without that setting, and building it costs every install a pass over the whole lockfile.
