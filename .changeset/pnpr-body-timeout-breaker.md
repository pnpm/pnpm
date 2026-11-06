---
"@pnpm/pnpr": patch
---

Response body timeouts no longer open an upstream's circuit breaker. Circuit-open responses name the upstream and include `Retry-After`.
