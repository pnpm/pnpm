---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fixed `pnpm publish` failing with a 404 when authentication relied on OIDC trusted publishing alongside an `.npmrc` written by `actions/setup-node` (`_authToken=${NODE_AUTH_TOKEN}`) without `NODE_AUTH_TOKEN` being set. Unresolved `${VAR}` placeholders in auth values are now treated as empty rather than passed through verbatim, so the literal placeholder no longer surfaces as a bearer token when OIDC fallback is the intended auth source [#11513](https://github.com/pnpm/pnpm/issues/11513).
