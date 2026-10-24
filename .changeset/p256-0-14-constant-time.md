---
"pacquet": patch
"@pnpm/pnpr": patch
---

pnpm and pnpr now sign shared artifacts, OCI registry tokens, and OIDC login state with a P-256 release that fixes a timing side channel. See [the advisory](https://rustsec.org/advisories/RUSTSEC-2026-0387).
