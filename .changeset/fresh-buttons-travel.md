---
"@pnpm/plugin-commands-installation": minor
pnpm: minor
---

Removed a branching code path that only executed when `dedupe-peer-dependents=false`. We believe this internal refactor will not result in behavior changes, but we expect it to make future pnpm versions behave more consistently for projects that override `dedupe-peer-dependents` to false. There should be less unique bugs from turning off `dedupe-peer-dependents`.

See details in [#9259](https://github.com/pnpm/pnpm/pull/9259).
