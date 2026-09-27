---
"@pnpm/releasing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm publish` now waits at least 5 minutes for the registry to answer a publish request, like npm. This fixes "409 Conflict - Failed to save packument" errors when the registry is slow to answer [pnpm/pnpm#11454](https://github.com/pnpm/pnpm/issues/11454).
