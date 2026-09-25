---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"pacquet": patch
"pnpm": patch
---

Commands run from a POSIX shell through a dependency's own `node_modules/.bin`, such as `node_modules/vite/node_modules/.bin/esbuild`, no longer fail with `MODULE_NOT_FOUND` [#10189](https://github.com/pnpm/pnpm/issues/10189).
