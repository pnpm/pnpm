---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

POSIX bin shims now take `cygpath` and `wslpath` from the system default path on Cygwin, MSYS2, and WSL2. The shims looked both helpers up on `PATH`, where a dependency's own bins come first, so a dependency could redirect another package's shim. Installing again replaces the shims already in `node_modules` [#14866](https://github.com/pnpm/pnpm/issues/14866).
