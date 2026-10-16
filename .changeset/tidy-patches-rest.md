---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

`verifyDepsBeforeRun` now compares a patch file that changed on disk with the patch hash in the lockfile. A patch that was copied or touched without changing its content no longer marks dependencies as outdated [#16988](https://github.com/pnpm/pnpm/issues/16988).
