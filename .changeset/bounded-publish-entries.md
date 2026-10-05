---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm publish` now rejects manifests and README files larger than 64 MiB in pre-built tarballs before reading them into memory.
