---
"pacquet": patch
---

`pnpm publish --provenance` now retries the sigstore signing exchange up to two more times with exponential backoff when a request to the sigstore infrastructure fails, instead of aborting the publish on the first transient network error.
