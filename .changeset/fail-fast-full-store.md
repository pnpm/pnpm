---
"@pnpm/network.fetch": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now reports a full content-addressable store without retrying the tarball when writing package files fails. Related to [pnpm/pnpm#8581](https://github.com/pnpm/pnpm/issues/8581).
