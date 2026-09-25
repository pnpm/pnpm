---
"@pnpm/bins.cmd-shim": patch
"pnpm": patch
"pacquet": patch
---

Bin shims in `node_modules/.bin` run from Cygwin on Windows again. The shims passed a `/cygdrive/c/...` path to the Windows `node` found on `PATH`, so Node.js failed with `Cannot find module 'C:\cygdrive\c\...'` [#12845](https://github.com/pnpm/pnpm/issues/12845).
