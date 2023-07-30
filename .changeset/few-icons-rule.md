---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

`pnpm dlx` should not print an error stack when the underlying script execution fails [#6698](https://github.com/pnpm/pnpm/issues/6698).
