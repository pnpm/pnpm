---
"@pnpm/installing.context": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

New workspace projects now prefer peer versions already pinned in the lockfile. Dependencies that are also optional peers no longer cause an unrelated project to select an older peer version.
