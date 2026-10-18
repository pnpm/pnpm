---
"@pnpm/pnpr": minor
---

The `/npm/-/tarballs/sha512/<digest>` route now names a package by the SHA-512 of its uncompressed tar, on `.tgz` and tar registries alike. It serves the tar with `gzip` or `zstd` content coding, or none, as the client's `Accept-Encoding` allows. pnpr records a hosted version's digest reference when the version is published. pnpr checks a proxied upstream revision against its digest before it caches or serves it.

pnpr now keeps `dist.revision: 0` for an original in upstream revision metadata. pnpr keeps revision metadata only when its history lists revision 0. A revision's `dist` omits `shasum` and `signatures`. Tar registries keep revision metadata.
