---
"@pnpm/store.cafs": patch
"@pnpm/worker": patch
"pnpm": patch
---

pnpm now uses less memory when installing a package whose archive is larger than 64 MiB unpacked. It decompresses such archives as a stream [#14164](https://github.com/pnpm/pnpm/issues/14164).
