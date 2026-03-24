---
"@pnpm/store.cafs": patch
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Improve headless cold install performance (~35% faster in benchmarks) by reducing filesystem syscalls during package extraction and import. Also fixes a bug where the CAS locker cache was not updated when a file already existed with correct integrity, causing repeated integrity re-verification on subsequent lookups.
