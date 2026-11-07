---
"@pnpm/pnpr": patch
---

Sped up publishing with an `Idempotency-Key` header by indexing recorded publication outcomes by commit time. pnpr now refuses SQLite and libSQL metadata databases whose schema version is not 0.4.
