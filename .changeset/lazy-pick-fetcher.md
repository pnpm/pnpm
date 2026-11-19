---
"@pnpm/installing.package-requester": patch
"pnpm": patch
---

Sped up repeat installs by deferring fetcher selection when a registry tarball already has integrity metadata [#12583](https://github.com/pnpm/pnpm/issues/12583).
