---
"pacquet": patch
---

Resolve `catalog:` specifiers in the dependencies of injected workspace packages (`injectWorkspacePackages: true`). Previously such a child spec bypassed catalog resolution and failed with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`, matching the TypeScript CLI.
