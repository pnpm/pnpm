---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.resolver-base": patch
"@pnpm/store.controller-types": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed `pnpm up <pkg>` producing a different result than a fresh install of the same manifests would. The resolver now distinguishes `updateRequested` (true only for packages that match the user's update target) from the broader `update` flag, and for the targeted package ignores only its own lockfile-derived preferred-version pins — so the target re-resolves exactly as if its lockfile entries were deleted and `pnpm install` ran. Preferred versions a fresh install applies (manifest pins, versions propagated down the dependency chain, and the vulnerability-avoidance penalties of `pnpm audit --fix`) stay in effect, so an update never installs duplicate versions that a reinstall from scratch would not reproduce. When a preferred version holds the update target below the newest version its range admits, pnpm now prints a warning explaining that reaching the newer version everywhere requires an override.
