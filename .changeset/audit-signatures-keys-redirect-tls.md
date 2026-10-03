---
"pacquet": patch
---

`pnpm audit signatures` now uses the TLS settings of the redirect target when a registry redirects its signing-keys request, for example to registry.npmjs.org. A `cafile` scoped to a private registry no longer makes the redirected request fail [#16541](https://github.com/pnpm/pnpm/issues/16541).
