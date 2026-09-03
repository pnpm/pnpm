---
"pacquet": patch
---

`pnpm install` no longer reruns root lifecycle scripts when the global virtual store contains an unfinished-build marker in a package slot that the current lockfile does not use [pnpm/pnpm#14485](https://github.com/pnpm/pnpm/issues/14485).
