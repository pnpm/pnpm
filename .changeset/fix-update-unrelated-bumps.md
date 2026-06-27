---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fixed `pnpm up -r <pkg>` bumping unrelated packages that have open semver ranges. Previously, any update mutation nullified the lockfile-derived `preferredVersions` globally, so packages with `^x.y.z` ranges could re-resolve to newer compatible versions even though the user only asked to update a specific package. The install layer now always seeds `preferredVersions` from the lockfile; the targeted package still bumps via the per-resolve `updateRequested` bypass introduced in the prior fix.

Closes pnpm/pnpm#10662.
