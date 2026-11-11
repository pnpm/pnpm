---
"@pnpm/pnpr": patch
---

Added the `maxTarballSize` setting, the largest tarball pnpr fetches from an upstream registry. It takes a byte size such as `256MiB`. The default is `100MiB`, the limit pnpr applied before. A tarball that declares a size over the limit is refused with a 502 status.
