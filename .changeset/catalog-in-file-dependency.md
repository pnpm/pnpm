---
"pacquet": patch
---

`pnpm install` now fails with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a `file:` dependency that is not a workspace project uses a `catalog:` specifier. pnpm resolved such specifiers from the workspace catalogs, unlike pnpm v11. Injected workspace projects still resolve their `catalog:` dependencies.
