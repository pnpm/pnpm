---
"@pnpm/resolving.local-resolver": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install` failing with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a workspace package injected through a relative path, such as `workspace:../lib`, has `catalog:` dependencies. Their catalog entries now resolve the way they do for a package injected by name [#17280](https://github.com/pnpm/pnpm/issues/17280).
