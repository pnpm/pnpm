---
"@pnpm/pnpr": minor
---

With SQLite metadata, pnpr commits a publish or batch publish as one transaction, so readers see all of it or none of it. A retried publish gets its original response instead of a 409, and batch publishes accept an `Idempotency-Key` header. Each publish is recorded as a publication, served at `/-/publications/<org>/<id-or-name>`.
