---
"@pnpm/pnpr": minor
---

pnpr serves Prometheus metrics at `/-/metrics` and `/metrics` only when the config has a `metrics` block. Before, it served them to anyone.

Set `metrics.token` to require a bearer token from the scraper. Set `metrics.listen` to serve metrics on a separate address, such as one on a private network, and not on the main address.
