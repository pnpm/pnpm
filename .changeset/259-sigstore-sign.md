---
"pacquet": patch
---

`pnpm publish` with provenance now downloads Sigstore's current signing configuration before signing. A failed or slow download is retried with the same retry policy and deadline as the signing requests.
