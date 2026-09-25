---
"@pnpm/building.during-install": patch
"pnpm": patch
"pacquet": patch
---

When a dependency's build script fails under `enableGlobalVirtualStore`, the global virtual store directory it was being built in is now removed for scoped packages too. Previously the cleanup resolved one directory level short of the hash directory for a scoped name, leaving a half-built directory behind that later installs would reuse.
