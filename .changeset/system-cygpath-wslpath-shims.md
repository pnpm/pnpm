---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

POSIX bin shims now convert Windows paths with the system `cygpath` and `wslpath` on Cygwin, MSYS2, and WSL. The shims looked those helpers up on `PATH`, which starts with `node_modules/.bin`, so a dependency shipping a bin under either name could redirect another package's shim. A host that has neither helper on the system default path still falls back to `PATH`. Installing again replaces the shims already in `node_modules` [#14866](https://github.com/pnpm/pnpm/issues/14866).
