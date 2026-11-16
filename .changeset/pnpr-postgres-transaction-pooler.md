---
"@pnpm/pnpr": patch
---

pnpr on PostgreSQL works behind PgBouncer and other connection pools that run in transaction mode. Writes from more than one pnpr instance stalled there until they timed out.
