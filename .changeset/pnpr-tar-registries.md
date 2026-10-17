---
"@pnpm/pnpr": minor
---

A registry with `tarballs: tar` serves each version as the uncompressed tar of its `.tgz`, compressed with zstd or gzip for the request, and its metadata pins the hash of the tar. Every registry now also serves a version's tarball at `<registry>/<name>@<version>`.
