---
"pacquet": patch
---

Restored reading and updating `package.json5` project manifests. Manifest updates retain comments, and workspace discovery prefers `package.json`, then `package.json5`, then `package.yaml` [pnpm/pnpm#15129](https://github.com/pnpm/pnpm/issues/15129).
