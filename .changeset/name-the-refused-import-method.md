---
"pacquet": patch
---

An install that fails because the filesystem cannot hard link or clone now names the `packageImportMethod` setting and the value to use instead. This only affects `hardlink` and `clone`, which report the failure rather than copying [pnpm/pnpm#14782](https://github.com/pnpm/pnpm/issues/14782).
