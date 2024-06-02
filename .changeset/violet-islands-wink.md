---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

`exec` now also streams prefixed output when `--recursive` or `--parallel` is specified just as `run` does [#8065](https://github.com/pnpm/pnpm/issues/8065).
