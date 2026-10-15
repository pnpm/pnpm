---
"@pnpm/pnpr": minor
---

pnpr now supports token creation via `POST /-/npm/v1/tokens` with `--read-only`, CIDR restrictions, expiry, and token naming. Expired tokens are rejected during authentication. Token authentication updates the `last_used_at` timestamp. Read-only tokens are permitted to access npm audit endpoints [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).
