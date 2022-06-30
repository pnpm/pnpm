---
"@pnpm/node.fetcher": patch
"pnpm": patch
---

`pnpm env use` should throw an error on a system that use the MUSL libc.
