---
"pacquet": patch
"@pnpm/pnpr": patch
---

Signing in pnpm and pnpr no longer uses arithmetic that may leak the private key through timing. This covers shared artifacts, OCI registry tokens, and OIDC login state [advisory](https://rustsec.org/advisories/RUSTSEC-2026-0387).
