---
"@pnpm/building.after-install": patch
"@pnpm/building.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/patching.config": patch
"pnpm": patch
---

Fixed `pnpm rebuild` modifying packages shared with projects that have not approved their build scripts when using the global virtual store.
