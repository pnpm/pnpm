---
"pacquet": patch
---

A dependency published with `"bin": ""`, such as `url-loader@1.1.2`, no longer fails the install with `ERR_PNPM_CMD_SHIM_PROBE_SHIM_SOURCE` [#13962](https://github.com/pnpm/pnpm/issues/13962). An empty `bin` declares no command, as it does in pnpm v11, so no shim is written for the package; a `directories.bin` entry on the same package is still linked.
