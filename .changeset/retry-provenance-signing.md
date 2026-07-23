---
"pacquet": patch
---

`pnpm publish --provenance` now applies the `fetch-timeout` setting to the sigstore signing exchange and retries it up to two more times with exponential backoff when it fails or times out, instead of aborting the publish on the first transient network error or hanging on a stalled connection.
