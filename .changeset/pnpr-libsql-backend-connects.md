---
"@pnpm/pnpr": patch
---

pnpr can now use a libsql or Turso database for users and tokens. Before, a `backend.libsql` block made pnpr crash at startup. pnpr checks the database's certificate against the system trust store.
