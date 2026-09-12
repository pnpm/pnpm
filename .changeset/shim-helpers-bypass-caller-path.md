---
"pacquet": patch
---

A dependency's own bins can no longer take over another package's bin shim. The POSIX shims pnpm generates used to look up their shell helpers on `PATH`, where a dependency's bins come first [#14837](https://github.com/pnpm/pnpm/issues/14837).
