---
"pacquet": patch
---

`pnpm install` now fails with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a dependency uses a specifier with a protocol pnpm does not support, such as Yarn's `patch:`. On Windows, such a specifier failed with `os error 123`. On other platforms, pnpm linked it to a directory that does not exist. Reading a `package.json` that fails now names the file [#16590](https://github.com/pnpm/pnpm/issues/16590).
