---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

Stopped `verifyDepsBeforeRun` from repeating a successful lockfile content check before every command in a single-project repository [#16993](https://github.com/pnpm/pnpm/issues/16993).
