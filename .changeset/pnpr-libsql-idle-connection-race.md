---
"@pnpm/pnpr": patch
---

Fixed occasional `500` errors with the code `ERR_PNPR_LIBSQL` on writes to a Turso database, such as a publish, that arrived about 5 seconds after the previous database request. pnpr no longer reuses a database connection that Turso may be closing.
