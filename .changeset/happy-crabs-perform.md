---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

`pnpm dlx` shouldn't modify the lockfile in the current working directory [#4743](https://github.com/pnpm/pnpm/issues/4743).
