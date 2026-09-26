---
"@pnpm/store.cafs": patch
"@pnpm/store.cafs-types": patch
"@pnpm/worker": patch
"pacquet": patch
"pnpm": patch
---

pnpm now skips side-effects caching for build outputs that contain symlinks [#12859](https://github.com/pnpm/pnpm/issues/12859).
