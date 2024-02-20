---
"@pnpm/store.cafs": patch
"pnpm": patch
---

Don't fail on a tarball that appears to be not a USTAR or GNU TAR archive. Still try to unpack the tarball [#7120](https://github.com/pnpm/pnpm/issues/7120).
