---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

`verifyDepsBeforeRun` no longer reports dependencies as out of sync, or installs before running a script, when only the `version` field of a single-project repository changed [#17560](https://github.com/pnpm/pnpm/issues/17560).
