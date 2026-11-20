---
"@pnpm/pnpr": patch
---

pnpr now compresses package metadata with zstd for clients that accept it. A full packument such as vite's is served in about 650 KB with zstd and 4.9 MB with gzip. pnpr also accepts zstd-compressed responses from upstream registries.
