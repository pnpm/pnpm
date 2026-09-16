---
"@pnpm/bins.cmd-shim": patch
"pnpm": patch
"pacquet": patch
---

Generated POSIX bin shims now try `cygpath` and `wslpath` on the system default path before falling back to the caller's `PATH` [pnpm/pnpm#14866](https://github.com/pnpm/pnpm/issues/14866).
