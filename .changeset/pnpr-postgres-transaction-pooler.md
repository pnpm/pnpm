---
"@pnpm/pnpr": patch
---

pnpr on PostgreSQL works behind a connection pooler in transaction mode, such as PgBouncer. Writes from more than one pnpr instance stalled there until they timed out.
