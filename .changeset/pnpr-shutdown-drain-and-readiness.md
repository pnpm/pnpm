---
"@pnpm/pnpr": patch
---

Added `shutdownTimeout` (`25s` by default) to configure how long pnpr waits for in-flight requests to drain on `SIGINT` or `SIGTERM`. `GET /-/ready` now returns `503 Service Unavailable` as soon as shutdown starts. `GET /-/ping` now returns `{}` without probing storage or auth backends.
