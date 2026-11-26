---
"pnpm": patch
---

pnpm now accepts tarballs with unambiguous multiline package metadata from older npm releases. When `pnpm store add` fails to fetch or extract a tarball, the warning identifies the archive [#18479](https://github.com/pnpm/pnpm/issues/18479).
