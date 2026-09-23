---
"@pnpm/store.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm store status` no longer falsely reports packages with build or postinstall scripts as modified in the store. Packages requiring builds have their integrity verified against recorded side effects, or are recognized as built packages.
