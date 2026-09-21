---
"pacquet": patch
---

Fixed repeated `pnpm install --no-runtime --frozen-lockfile` failing with a broken lockfile when using `nodeLinker: hoisted` [#15212](https://github.com/pnpm/pnpm/issues/15212).
