---
"pacquet": patch
---

Fixed `pnpm install --frozen-lockfile` rejecting a lockfile that pnpm had just generated. The rejection happened when `pnpm.overrides` pointed a dependency at a relative `file:` or `link:` path and the workspace had projects below the lockfile directory [#14555](https://github.com/pnpm/pnpm/issues/14555).
