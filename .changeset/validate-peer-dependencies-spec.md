---
"pacquet": patch
---

`pnpm install` now validates `peerDependencies` specifiers and fails with `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION` when a value is not a valid range or specifier [https://github.com/pnpm/pnpm/issues/14791](https://github.com/pnpm/pnpm/issues/14791).
