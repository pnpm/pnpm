---
"@pnpm/fetching.tarball-fetcher": patch
"pnpm": patch
---

Local tarball dependencies using the file protocol are no longer counted as downloaded in the progress banner [pnpm/pnpm#1103](https://github.com/pnpm/pnpm/issues/1103).
