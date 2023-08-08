---
"@pnpm/tarball-fetcher": patch
"pnpm": patch
---

Performance optimizations. Package tarballs are now download directly to memory and built to an ArrayBuffer. Hashing and other operations are avoided until the stream has been fully received [#6819](https://github.com/pnpm/pnpm/pull/6819).
