---
"@pnpm/pnpr": patch
---

pnpr now reuses its HTTP connections to a remote libsql or Turso auth database. Each token lookup opened a new connection before, and against Turso it paid a new TLS handshake.
