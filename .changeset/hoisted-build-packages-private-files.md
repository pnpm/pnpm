---
"pacquet": patch
---

With `nodeLinker: hoisted`, a package that runs a lifecycle script or has a patch is now cloned or copied into `node_modules`, as with the isolated linker. A build that rewrites one of its files no longer changes the store copy shared with other projects.
