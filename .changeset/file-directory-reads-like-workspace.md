---
"pacquet": minor
---

A `file:` dependency on a local directory now resolves `catalog:` specifiers in that directory's `package.json` against the workspace catalogs, as an injected workspace package does. Before, the install failed with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`.

A `file:` dependency on the `publishConfig.directory` of a project is now described by that project's `package.json`, as an injected workspace package is. A `package.json` that a build writes into that directory no longer changes the resolution.
