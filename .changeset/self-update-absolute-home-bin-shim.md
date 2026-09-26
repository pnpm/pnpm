---
"pacquet": patch
---

`pnpm self-update` writes the pnpm shim in the pnpm home bin directory as a resolved absolute path, with no `..` segments [pnpm/pnpm#12865](https://github.com/pnpm/pnpm/issues/12865).
