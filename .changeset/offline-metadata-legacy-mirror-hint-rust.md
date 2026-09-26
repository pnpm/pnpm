---
"pacquet": patch
---

If an offline install fails because the registry metadata cache uses the layout from before pnpm 11.27 and 12.4, the error now names the older mirror on disk and explains that one online install repopulates the cache. The error also carries the `ERR_PNPM_NO_OFFLINE_META` code [#15656](https://github.com/pnpm/pnpm/issues/15656).
