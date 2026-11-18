---
"@pnpm/pnpr": patch
---

Fixed writes on PostgreSQL waiting until they timed out when more than one pnpr instance wrote through PgBouncer or another connection pool in transaction mode.
