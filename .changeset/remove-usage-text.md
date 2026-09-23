---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm remove --help` no longer shows a `[@<version>]` suffix in its usage line. The command accepts package names only [#7751](https://github.com/pnpm/pnpm/issues/7751).
