---
"pacquet": patch
---

During dependency filtering in commands such as `pnpm install --prod --frozen-lockfile`, pnpm now recovers hashed peers in v9 lockfiles only when a binding can affect which dependencies are kept [#18276](https://github.com/pnpm/pnpm/issues/18276).
