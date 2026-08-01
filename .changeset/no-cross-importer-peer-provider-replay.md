---
"pacquet": patch
---

Fixed peer resolution creating far more peer variants than the TypeScript CLI in multi-importer workspaces: a dependency subtree first resolved under one importer no longer hands the peer providers it resolved to every other importer that shares it. Those importers now bind such peers against their own context (or the workspace root), matching the TypeScript resolver. In a large bit.cloud workspace this cut a from-scratch install from 25,534 to 20,791 lockfile snapshots.
