---
"@pnpm/building.during-install": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fixed `strictDepBuilds` and `allowBuilds` checks being bypassed when a package's build side-effects are cached in the store. Packages with cached builds were skipped by `buildModules` (`isBuilt: true`) and never reached the `allowBuild` check. Now checks `allowBuild` for all packages with `requiresBuild` regardless of `isBuilt` state. Also detects packages whose build approval was revoked between installs.
