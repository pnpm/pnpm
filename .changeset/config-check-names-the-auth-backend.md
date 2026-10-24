---
"@pnpm/pnpr": patch
---

`pnpr config check` now names the configured auth backend, such as libsql, PostgreSQL, or MySQL. It printed `htpasswd` for every backend.

`pnpr config check` now lists the default registry of every ecosystem. It listed only the npm default registry.
