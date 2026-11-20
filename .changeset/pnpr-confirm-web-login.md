---
"@pnpm/pnpr": patch
---

`pnpm login` and `npm login` against pnpr now print a code that the user types in the browser before signing in. A login link alone can no longer sign anyone in.

A token created without a password, by an account that has none, now expires no later than the session or token that created it. It also stays within that credential's CIDR whitelist.
