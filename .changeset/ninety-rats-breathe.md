---
"@pnpm/reviewing.dependencies-hierarchy": patch
---

Move loading `wantedLockfile` outside `dependenciesHierarchyForPackage`, preventing OOM crash when loading the same lock file too many times
