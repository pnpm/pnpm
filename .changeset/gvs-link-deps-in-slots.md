---
"pacquet": patch
---

A `link:` dependency of a package in the virtual store is now materialized inside that package's slot, and the slot is scoped to the directory the link resolves to.

A package whose peer dependency is satisfied by a `link:` — a workspace sibling, or a toolchain linked in from outside the workspace — records that edge in the lockfile (`@pnpm.e2e/peer-a: link:packages/peer-a`) but never got a symlink for it. Project-locally the omission was invisible: the slot sits under the importer's `node_modules`, so Node's upward walk reached the importer's own copy of the link. Under `enableGlobalVirtualStore` the slot lives in the shared store, that walk never reaches the project, and the dependency was missing at runtime with `Cannot find module`.

The same edge was also dropped from the global-virtual-store hash, so two projects that linked *different* directories collapsed onto one slot and shared whichever symlink was written first. Slots are now scoped by the resolved link target rather than by the project, so projects linking the same directory still share a slot.
