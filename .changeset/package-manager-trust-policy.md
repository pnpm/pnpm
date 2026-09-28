---
"pnpm": patch
"pacquet": patch
---

`pnpm run` failed when `packageManager` pinned an older pnpm and the project set `trustPolicy` to `no-downgrade`. Switching to the pinned version ignores the project's `trustPolicy` [pnpm/pnpm#16319](https://github.com/pnpm/pnpm/issues/16319).
