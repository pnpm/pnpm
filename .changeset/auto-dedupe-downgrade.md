---
"pacquet": patch
---

With `autoDedupe` enabled, downgrading a dependency in one workspace project now moves the other projects to that version when it satisfies their ranges. This also applies to a filtered `pnpm --filter <project> add` [#16432](https://github.com/pnpm/pnpm/issues/16432).
