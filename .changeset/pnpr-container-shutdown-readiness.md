---
"@pnpm/pnpr": minor
---

pnpr now responds to Unix `SIGTERM` signals with graceful shutdown, stopping new connections and completing in-flight requests before exiting. Dedicated readiness (`/-/ready`) and liveness (`/-/live`) endpoints are now available for container orchestration, and `/-/ping` returns 503 when backend storage or auth is inaccessible [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).
