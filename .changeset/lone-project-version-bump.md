---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

`verifyDepsBeforeRun` no longer reports dependencies as out of sync when only the `version` field of a single-project repository changed. A script run after such a change no longer starts an install first [#17560](https://github.com/pnpm/pnpm/issues/17560).
