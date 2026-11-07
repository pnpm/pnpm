---
"@pnpm/pnpr": patch
---

Sped up publishing with an `Idempotency-Key` header. pnpr now refuses SQLite and libSQL metadata databases whose schema version is not 0.4 [zkochan/pnpm-tasks#36](https://github.com/zkochan/pnpm-tasks/issues/36).
