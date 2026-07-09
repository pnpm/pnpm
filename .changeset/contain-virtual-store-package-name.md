---
"@pnpm/deps.graph-builder": patch
"@pnpm/lockfile.to-pnp": patch
"pnpm": patch
---

Prevent a crafted `pnpm-lock.yaml` from writing package content outside the virtual store. A dependency path key whose name reconstructs to a path-traversal sequence (e.g. `../../../tmp/x@1.0.0`) is now rejected by the isolated (virtual-store) linker and the Plug'n'Play resolver map, matching the containment already applied to the hoisted linker.
