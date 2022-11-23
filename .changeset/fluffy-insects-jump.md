---
"@pnpm/plugin-commands-publishing": patch
"pnpm": patch
---

`pnpm publish <tarball path>` should exit with non-0 exit code when publish fails [#5396](https://github.com/pnpm/pnpm/issues/5396).
