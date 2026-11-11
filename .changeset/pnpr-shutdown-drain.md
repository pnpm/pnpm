---
"@pnpm/pnpr": patch
---

pnpr now limits graceful shutdown to 30 seconds after receiving SIGINT or SIGTERM. Slow clients can no longer delay process termination indefinitely.
