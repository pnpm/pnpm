---
"pnpm": patch
---

`pnpm run` no longer sends a script a second `SIGINT` when `Ctrl+C` is pressed in a terminal. A script that shuts down on the first `SIGINT` and exits at once on a second used to die before its shutdown finished [#7374](https://github.com/pnpm/pnpm/issues/7374).
