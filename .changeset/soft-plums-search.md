---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Skip reimporting packages into the global virtual store when the content-addressed target already has its completion marker. This avoids redundant file work on fresh installs that reuse an already-warm `enableGlobalVirtualStore` links directory [#11112](https://github.com/pnpm/pnpm/issues/11112).
