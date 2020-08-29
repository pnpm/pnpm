---
"supi": patch
---

When updating specs in the lockfile, read the specs from the manifest in the right order: optionalDependencies > dependencies > devDependencies.
