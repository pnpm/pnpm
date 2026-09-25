---
"pacquet": minor
---

`pnpm install` now supports the opt-in `preserveBinName` setting. On POSIX, Node.js bin scripts use the normal shell shim and report the invoked command name in `process.argv[1]`, even when `preferSymlinkedExecutables` is enabled [#1311](https://github.com/pnpm/pnpm/issues/1311).
