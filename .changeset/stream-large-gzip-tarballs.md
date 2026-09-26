---
"@pnpm/store.cafs": patch
"@pnpm/worker": patch
"pnpm": patch
---

pnpm now decompresses a package archive larger than 64 MiB unpacked as a stream. Peak memory during extraction is bounded by the package's largest file, not its whole unpacked size [#14164](https://github.com/pnpm/pnpm/issues/14164).
