---
"@pnpm/pnpr": patch
---

With PostgreSQL or MySQL metadata, concurrent publishes and OCI blob uploads no longer fail with HTTP 500. Metadata writes now wait for each other. Before, they failed with a serialization error or a deadlock. Database errors from PostgreSQL and MySQL are now logged as `SQL database error`, not `Auth database error`.
