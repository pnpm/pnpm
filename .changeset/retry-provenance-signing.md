---
"pacquet": patch
---

`pnpm publish --provenance` now retries the sigstore signing exchange (Fulcio, timestamp authority, Rekor) up to two more times with exponential backoff when it fails, instead of aborting the publish on the first transient network error.
