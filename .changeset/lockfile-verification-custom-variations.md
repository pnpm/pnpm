---
"@pnpm/installing.deps-installer": patch
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Lockfile verification now validates inner resolutions within variations wrappers and rejects custom resolutions under registry-shaped dependency paths.
