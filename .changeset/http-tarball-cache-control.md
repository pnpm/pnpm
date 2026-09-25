---
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/installing.client": patch
"@pnpm/resolving.default-resolver": patch
"@pnpm/resolving.tarball-resolver": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` honors `Cache-Control` for dependencies named with an `http:` or `https:` tarball URL. A fresh response is taken from the store with no request, and a stale one is revalidated with `If-None-Match`.

https://github.com/pnpm/pnpm/issues/15648
