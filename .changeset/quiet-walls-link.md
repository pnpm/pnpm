---
"@pnpm/store.cafs": patch
"@pnpm/store.cafs-types": patch
"@pnpm/worker": patch
"pacquet": patch
"pnpm": patch
---

Skip side-effects caching for build outputs that contain symlinks so warm installs recreate the links instead of restoring regular file copies [#12859](https://github.com/pnpm/pnpm/issues/12859).
