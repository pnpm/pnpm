---
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

POSIX bin shims now preserve Windows-style paths during path normalization. Reinstalling pnpm replaces existing shims in `node_modules`. See [#14867](https://github.com/pnpm/pnpm/issues/14867).
