---
"pacquet": patch
---

`pnpm install` now fails with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a `file:` dependency that is not a workspace project uses a `catalog:` specifier. Injected workspace projects still resolve their `catalog:` dependencies.
