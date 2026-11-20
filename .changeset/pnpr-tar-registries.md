---
"@pnpm/pnpr": minor
---

A registry with `tarballs: tar` serves each version as the uncompressed tar of its `.tgz`. pnpr compresses the tar with zstd or gzip, as the request accepts. The registry's metadata pins the hash of the tar.

Every registry now also serves a version's tarball at `<registry>/<name>@<version>`.
