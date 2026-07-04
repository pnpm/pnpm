---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm update <dep>@<version>` now prints a warning when `<dep>` is only present as a transitive dependency: the requested version cannot be applied there (updates resolve the target the way a fresh install would), and the warning recommends adding the version to `pnpm.overrides` instead, which is the mechanism that does pin transitive dependencies. Closes pnpm/pnpm#12744.
