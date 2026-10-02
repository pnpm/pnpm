---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

pnpm no longer downloads every packument again on each install from a registry whose metadata responses forbid caching, such as `Cache-Control: no-store`. pnpm revalidates the cached metadata with a conditional request, so a registry that supports conditional requests answers with a 304 when the package has not changed [#16528](https://github.com/pnpm/pnpm/issues/16528).
