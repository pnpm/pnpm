---
"supi": patch
---

Fixing a regression that was shipped with supi v0.41.22. Cyclic dependencies that have peer dependencies are not symlinked to the root of node_modules, when they are direct dependencies.
