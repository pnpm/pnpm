---
"@pnpm/pnpr": patch
---

pnpr now gives up on a TCP connection attempt to the libsql auth database after 10 seconds. At startup it retries an attempt that timed out until the startup deadline, so a dropped connection attempt no longer stops the server.
