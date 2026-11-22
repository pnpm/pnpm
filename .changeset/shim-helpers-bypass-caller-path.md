---
"pacquet": patch
---

A dependency's own bins can no longer take over another package's bin shim. The POSIX shims pnpm generates used to look up their shell helpers on `PATH`, where a dependency's bins come first [#14837](https://github.com/pnpm/pnpm/issues/14837). Reinstalling replaces the shims already in your `node_modules`. On Cygwin, MSYS2, and WSL the shims still take their Windows path conversion from `PATH`, so a dependency can still redirect them there.
