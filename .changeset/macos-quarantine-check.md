---
"pnpm": patch
---

On macOS, `pnpm install` now runs `xattr` only for native binaries that carry the quarantine attribute [#18469](https://github.com/pnpm/pnpm/issues/18469).
