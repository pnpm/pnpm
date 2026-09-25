---
"@pnpm/fetching.tarball-fetcher": patch
"pnpm": patch
---

`pnpm import` and fresh resolutions now record `integrity` for git-hosted tarballs, such as `codeload.github.com` URLs, even when the tarball is already in the store [#13338](https://github.com/pnpm/pnpm/issues/13338).
