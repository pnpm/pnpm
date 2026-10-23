---
"@pnpm/pnpr": patch
---

pnpr now gives up on a connection to the libsql auth database after 10 seconds. At startup it retries a connection that timed out until the startup deadline, so a dropped connection attempt no longer stops the server.
