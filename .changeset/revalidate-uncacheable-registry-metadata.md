---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Installs from a registry whose metadata responses forbid caching, such as `Cache-Control: no-store`, no longer download every packument again on each run. pnpm revalidates the cached metadata with `Cache-Control: no-cache`, so the registry answers with a 304 when the package has not changed [#16528](https://github.com/pnpm/pnpm/issues/16528).
