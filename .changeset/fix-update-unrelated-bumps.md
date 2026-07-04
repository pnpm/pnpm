---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fixed `pnpm up -r <pkg>` bumping unrelated packages that have open semver ranges. Previously, any update mutation nullified the lockfile-derived `preferredVersions` globally, so packages with `^x.y.z` ranges could re-resolve to newer compatible versions even though the user only asked to update a specific package. The install layer now always seeds `preferredVersions` from the lockfile, and caller-supplied preferred versions (such as the vulnerability penalties of `pnpm audit --fix`) layer on top of the seed instead of replacing it. The targeted package still bumps: the per-resolve `updateRequested` flag makes the resolver ignore the target's own lockfile pins.

Closes pnpm/pnpm#10662.
