---
"@pnpm/pnpr": minor
---

pnpr now supports the npm audit endpoints: `POST /-/npm/v1/security/advisories/bulk`, `POST /-/npm/v1/security/audits`, and `POST /-/npm/v1/security/audits/quick`. Bulk advisory queries evaluate against local records when an OSV database is enabled. Requests forward to configured upstream registries when OSV is disabled [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).
