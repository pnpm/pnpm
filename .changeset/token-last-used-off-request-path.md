---
"@pnpm/pnpr": patch
---

A token-authenticated request waited for the token's last-used time to be written to the auth database before it was handled. The server now writes the time in the background, at most once a minute per token, so a request against a shared or replicated database no longer pays a database round trip for it. The time `GET /-/npm/v1/tokens` reports is accurate to within a minute [pnpm/tasks#194](https://github.com/pnpm/tasks/issues/194).
