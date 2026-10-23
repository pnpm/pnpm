---
"@pnpm/pnpr": minor
---

pnpr no longer serves Prometheus metrics to anyone who asks. `/-/metrics` and `/metrics` are off unless the config has a `metrics` block.

Set `metrics.token` to require a bearer token from the scraper. Set `metrics.listen` to serve metrics on a separate address, such as one on a private network, and not on the main address.
