---
"pacquet": patch
---

A dependency published with `"bin": ""` no longer fails the whole install with `ERR_PNPM_CMD_SHIM_PROBE_SHIM_SOURCE` [#13962](https://github.com/pnpm/pnpm/issues/13962). An empty `bin` declares no command, as pnpm treats it, so no shim is created for it; a `directories.bin` entry on the same package is still linked.
