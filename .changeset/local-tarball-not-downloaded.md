---
"@pnpm/fetching.tarball-fetcher": patch
"pnpm": patch
---

Local tarball dependencies (`file:` protocol) are no longer counted as "downloaded" in the progress banner.
