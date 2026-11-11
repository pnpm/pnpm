---
"@pnpm/pnpr": patch
---

pnpr limits upstream package metadata responses with `maxPackumentSize`, which defaults to 32 MiB. Cached tarballs stop being served when refreshed metadata removes their versions. Missing tarball integrities are computed in the background and retained across matching refreshes.
