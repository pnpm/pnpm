---
"@pnpm/installing.commands": major
"@pnpm/releasing.commands": major
"@pnpm/patching.commands": major
"@pnpm/bins.package-bins": major
"@pnpm/patching.apply-patch": major
"@pnpm/patching.config": major
"@pnpm/patching.types": major
"@pnpm/installing.deps-restorer": major
"@pnpm/building.during-install": major
"@pnpm/installing.deps-installer": major
"@pnpm/types": major
"@pnpm/config.reader": major
"pnpm": major
---

Removed the deprecated `allowNonAppliedPatches` completely in favor of `allowUnusedPatches`.
Remove `ignorePatchFailures` so all patch application failures should throw an error.
