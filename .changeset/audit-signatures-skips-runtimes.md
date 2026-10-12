---
"pacquet": patch
---

`pnpm audit` and `pnpm audit signatures` no longer check runtimes installed through `devEngines.runtime`, such as `node@runtime:24.21.0`, against the npm package of the same name. `pnpm audit signatures` reported these runtimes as having an invalid registry signature [#16869](https://github.com/pnpm/pnpm/issues/16869).
