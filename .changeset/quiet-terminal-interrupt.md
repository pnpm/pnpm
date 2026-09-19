---
"pacquet": patch
---

`pnpm run` no longer sends a script a second `SIGINT` when `Ctrl+C` is pressed in a terminal. The terminal already interrupts the script along with pnpm, so a script that shuts down on the first `SIGINT` and exits at once on a second now gets to finish its shutdown [#7374](https://github.com/pnpm/pnpm/issues/7374).
