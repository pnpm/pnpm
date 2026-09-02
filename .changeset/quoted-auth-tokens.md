---
"pacquet": patch
---

Fixed pnpm retaining the surrounding quotes in `.npmrc` values, including auth tokens expanded from environment variables. This restores authentication with registries configured using `:_authToken="${TOKEN}"` [pnpm/pnpm#14427](https://github.com/pnpm/pnpm/issues/14427).
