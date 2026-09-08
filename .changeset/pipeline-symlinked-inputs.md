---
"pacquet": patch
---

`pnpm pipeline` no longer fails on a project that tracks a symlink, such as a `CLAUDE.md` pointing at `AGENTS.md`. Changing a symlinked input's target invalidates that task's cache. `pnpm pipeline --no-cache` no longer hashes task inputs [#14692](https://github.com/pnpm/pnpm/issues/14692).
