---
"pacquet": patch
---

Workspace project discovery no longer descends into pnpm-managed directories. When the store, cache, or state directory lives inside the workspace (for example an in-workspace `storeDir`), manifests found there were reported as workspace projects, so their lifecycle scripts could run outside the `allowBuilds` approval gate. Discovery now skips the store, cache, state, modules, and virtual-store directories.

https://github.com/pnpm/pnpm/issues/15033
