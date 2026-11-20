---
"@pnpm/pnpr": patch
---

OIDC workload credentials from GitHub Actions now verify. Discovery failed for issuers that publish no `authorization_endpoint`, so every such request answered 500 [#17300](https://github.com/pnpm/pnpm/issues/17300).
