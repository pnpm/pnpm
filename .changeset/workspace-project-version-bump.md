---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

`verifyDepsBeforeRun` no longer reports dependencies as out of sync when only the `version` field of a workspace project changed and no dependency can resolve to that project. A script run after such a change no longer starts an install first.
