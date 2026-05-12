---
"@pnpm/network.fetch": patch
"pnpm": patch
---

Strip `sec-fetch-*` headers from outgoing HTTP requests. These headers are automatically added by undici's `fetch()` implementation per the Fetch spec but cause Azure DevOps Artifacts to return HTTP 400 for uncached upstream packages, as ADO interprets them as browser requests [#11572](https://github.com/pnpm/pnpm/issues/11572).
