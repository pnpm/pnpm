---
"@pnpm/tarball-fetcher": patch
"pnpm": patch
---

Don't retry fetching missing packages, since the retries will never work
