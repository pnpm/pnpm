---
"pacquet": patch
---

Resolving Yarn 6 authenticates its GitHub releases-API request with `GH_TOKEN` / `GITHUB_TOKEN` when one is set. The release list is the one unconditional GitHub API call the resolver makes, and the anonymous rate limit is counted per IP address — which CI runners share — so provisioning `yarn@6` in CI could fail with `ERR_PNPM_YARN_RELEASES_STATUS` no matter how rarely a single job asked. Exporting the token CI already has lifts the request onto the authenticated limit. The token is withheld when the project turns off `strict-ssl`, so relaxing certificate verification for a registry never sends a GitHub credential over the connection it relaxed.
