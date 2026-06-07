---
"@pnpm/config.commands": patch
"@pnpm/config.reader": patch
"pnpm": patch
---

Fixed `pnpm config get globalconfig` to return the global `config.yaml` path again [pnpm/pnpm#11962](https://github.com/pnpm/pnpm/issues/11962).
