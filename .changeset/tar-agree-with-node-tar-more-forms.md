---
"pnpm": patch
"@pnpm/pnpr": patch
---

pnpm now refuses more package tarballs that npm reads differently. Among them are a tarball whose tar continues into a second gzip member and a tar header whose checksum field has no terminator. Such a tarball could install different files with pnpm than with npm.
