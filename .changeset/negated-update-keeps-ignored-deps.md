---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update` with only negated selectors, such as `pnpm update '!foo'`, now leaves the dependencies listed in `update.ignoreDeps` unchanged [pnpm/tasks#243](https://github.com/pnpm/tasks/issues/243).
