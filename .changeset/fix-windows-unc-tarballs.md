---
"@pnpm/resolving.local-resolver": patch
"pnpm": patch
---

`pnpm install` now resolves local tarballs specified with bare UNC paths on Windows [pnpm/pnpm#1669](https://github.com/pnpm/pnpm/issues/1669).
