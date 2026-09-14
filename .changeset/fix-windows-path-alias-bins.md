---
"@pnpm/exe": patch
"pnpm": patch
"pacquet": patch
---

`pn`, `pnpx`, `pnx`, and the `pnpm` placeholder script now handle a native Windows path in `$0` when launched under Git Bash or MSYS2 [`pnpm/pnpm#14884`](https://github.com/pnpm/pnpm/issues/14884).
