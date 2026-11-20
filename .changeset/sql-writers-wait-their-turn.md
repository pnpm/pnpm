---
"@pnpm/pnpr": patch
---

With PostgreSQL or MySQL metadata, concurrent publishes and OCI blob uploads no longer fail with HTTP 500. Metadata writes now wait for each other. Concurrent writes used to fail with a serialization error or a deadlock. pnpr now logs PostgreSQL and MySQL errors as `SQL database error`.
