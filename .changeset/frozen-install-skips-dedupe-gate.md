---
"pacquet": patch
---

After `pnpm install --frozen-lockfile`, `pnpm run` and `pnpm exec` no longer start another install only because `autoDedupe` is enabled [#16583](https://github.com/pnpm/pnpm/issues/16583).
