---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

A repeat `pnpm install` no longer reruns the failed build of an optional dependency. It reports "Already up to date" when nothing else changed [#16468](https://github.com/pnpm/pnpm/issues/16468).
