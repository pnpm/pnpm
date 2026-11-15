---
"@pnpm/pnpr": minor
---

Tokens from a password login, and tokens from `npm token create` without an expiry, now expire after 90 days. The `auth.tokens.defaultLifetime` setting changes this lifetime.

Added the `auth.tokens.maxLifetime` setting. It limits how long any token from a password login or `npm token create` stays valid, for example `90d`.
