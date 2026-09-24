---
"pacquet": patch
---

`pnpm install` uses less CPU when it links packages from a warm store. On Windows, a warm install could take several times longer than with pnpm 11 [#15439](https://github.com/pnpm/pnpm/issues/15439).
