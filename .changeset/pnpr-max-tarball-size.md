---
"@pnpm/pnpr": patch
---

Added the `maxTarballSize` setting, the largest tarball pnpr fetches from an upstream registry. It takes a byte size such as `256MiB` and defaults to `100MiB`, which was the fixed limit before. A tarball over the limit is refused with a 502 status.
