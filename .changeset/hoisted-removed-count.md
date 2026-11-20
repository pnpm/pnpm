---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` with `nodeLinker: hoisted` no longer counts missing packages as removed. This fixes inflated removal counts for skipped optional dependencies [pnpm/pnpm#18178](https://github.com/pnpm/pnpm/issues/18178).
