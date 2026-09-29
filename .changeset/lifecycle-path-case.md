---
"pacquet": patch
---

On macOS and Linux, lifecycle scripts and `pnpm run` now always get `PATH` from the `PATH` variable. When the environment also held a `Path` variable, a script sometimes got `Path`'s value, and failed with `node: not found` [#16308](https://github.com/pnpm/pnpm/issues/16308).
