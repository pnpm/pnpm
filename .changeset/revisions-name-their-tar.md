---
"@pnpm/pnpr": minor
---

The `-/tarballs/sha512/<digest>` route now names a package by the SHA-512 of its uncompressed tar, on `.tgz` and tar registries alike. It serves the tar with `gzip` or `zstd` content coding, or none, as the client's `Accept-Encoding` allows. A hosted version gets its digest reference at publish. A proxied upstream revision is checked against its digest before it is cached or served.

Upstream revision metadata now keeps `dist.revision: 0` for an original, and is kept only with a history that lists revision 0. A revision's `dist` omits `shasum` and `signatures`. Tar registries keep revision metadata.
