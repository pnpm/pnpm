---
"pacquet": patch
---

The POSIX bin shims pnpm generates now take `readlink`, `sed`, and `uname` from the system default path instead of the caller's `PATH`, and no longer run `dirname` at all. A dependency's own bin could previously stand in for one of these and redirect the shim before it reached its target, because `pnpm run` puts `node_modules/.bin` first on `PATH` [#14837](https://github.com/pnpm/pnpm/issues/14837).
