---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

`pnpm exec` should exit with the exit code of the child process. This fixes a regression introduced in pnpm v6.20.4 via [#3951](https://github.com/pnpm/pnpm/pull/3951).
