---
"pacquet": patch
---

`pnpm install` and `pnpm publish` now reject archive metadata larger than 64 MiB before reading it into memory. Publishing a pre-built tarball also rejects manifests and README files larger than 64 MiB.
