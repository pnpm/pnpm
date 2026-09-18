---
"@pnpm/bins.cmd-shim": patch
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

A dependency's own bins can no longer take over another package's bin shim on Cygwin, MSYS2, and WSL. The POSIX shims pnpm generates looked up `cygpath` and `wslpath` on `PATH`, where a dependency's bins come first. They now try the system copy first and fall back to `PATH` only when it does not answer. Installing again replaces the shims already in `node_modules` [#14866](https://github.com/pnpm/pnpm/issues/14866).
