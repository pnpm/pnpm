---
"pacquet": patch
---

`pnpm deploy --legacy` no longer fails with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` when a workspace dependency of the deployed project declares a `workspace:` peer dependency that the project does not depend on itself. pnpm also no longer installs a registry package of the same name for such a peer. The peer is reported as missing [#16806](https://github.com/pnpm/pnpm/issues/16806).
