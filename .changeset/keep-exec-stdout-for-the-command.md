---
"pacquet": patch
---

The install that `verifyDepsBeforeRun` spawns before `pnpm run` / `pnpm exec` now reports to stderr, so lines such as `Scope: all 56 workspace projects` no longer land in the middle of the executed command's stdout and break a pipe into `jq` [#14197](https://github.com/pnpm/pnpm/issues/14197). `pnpm exec --silent` now suppresses that install's output too.
