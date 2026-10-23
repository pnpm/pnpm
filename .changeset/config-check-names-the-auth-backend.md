---
"@pnpm/pnpr": patch
---

`pnpr config check` now names the configured auth backend, such as libsql, PostgreSQL, or MySQL. It also lists the default registry of every ecosystem. It printed `htpasswd` for every backend and only the npm default registry.
