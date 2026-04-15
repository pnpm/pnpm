---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

Fix strictDepBuilds/allowBuilds not detecting unapproved build scripts when sharedWorkspaceLockfile is false. Closes #9082.
