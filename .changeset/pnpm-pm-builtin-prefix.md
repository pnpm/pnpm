---
"pnpm": minor
---

Added support for `pnpm pm <command>` to force running the built-in pnpm command, bypassing any same-named script in package.json. For example, `pnpm pm clean` always runs the built-in clean command even if a "clean" script exists.
