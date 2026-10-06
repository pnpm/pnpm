---
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/store.cafs": patch
"@pnpm/worker": patch
"pnpm": patch
---

Large package downloads and large files inside gzip and bzip2 package archives now use bounded memory during installation. Package manifests and archive metadata larger than 64 MiB are rejected.
