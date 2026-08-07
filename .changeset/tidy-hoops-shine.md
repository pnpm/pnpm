---
"pacquet": patch
---

Reduced peak memory usage and allocation churn during peer dependency resolution: peer-dependency issues now keep a cheap shared handle to their parent chain instead of a materialized copy per occurrence [#13681](https://github.com/pnpm/pnpm/issues/13681).
