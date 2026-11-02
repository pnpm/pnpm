---
"@pnpm/pnpr": minor
---

With SQLite metadata, a reader sees all of a publish or batch publish, or none of it. A retried publish gets its original response instead of a 409. Batch publishes accept an `Idempotency-Key` header. pnpr records each publish as a publication and serves it at `/-/publications/<org>/<id-or-name>`.
