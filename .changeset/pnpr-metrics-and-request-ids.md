---
"@pnpm/pnpr": minor
---

pnpr now exposes Prometheus metrics at `GET /-/metrics` and `GET /metrics`, tracks request IDs, and enriches access logs [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131). The metrics endpoint emits standard Prometheus text format with in-flight gauges, request counters, duration histograms, and cache statistics. Requests forward and echo `x-request-id` headers or receive generated UUIDs. Access logs under `pnpr::access` record request IDs, authenticated usernames, client IPs, user agents, status codes, latencies, and byte counts.
