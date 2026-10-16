---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` no longer include editor swap files (`.*.swp`), macOS `._*` files, `.lock-wscript`, `.wafpickle-*`, `build/config.gypi`, or `archived-packages/` in the tarball. Naming one of these files in `files` still includes it [#16955](https://github.com/pnpm/pnpm/issues/16955).
