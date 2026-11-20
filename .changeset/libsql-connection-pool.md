---
"@pnpm/pnpr": patch
---

pnpr no longer loses its libsql or Turso auth database after a request is cut off. Before, a client that disconnected during a token lookup could make every later login and authenticated request fail with a 500 until pnpr restarted. The new `backend.libsql.maxConnections` setting limits how many auth database operations run at once, and defaults to 10.
