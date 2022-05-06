---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

`pnpm dlx` should work when the bin name of the executed package isn't the same as the package name [#4672](https://github.com/pnpm/pnpm/issues/4672).
