---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install` with `nodeLinker: hoisted` counting skipped optional packages that were never installed as removed [pnpm/pnpm#18178](https://github.com/pnpm/pnpm/issues/18178).
