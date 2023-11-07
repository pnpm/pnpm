---
"@pnpm/tarball-fetcher": patch
"pnpm": patch
---

Don't retry fetching missing packages, since the retries will never work [#7276](https://github.com/pnpm/pnpm/pull/7276).
