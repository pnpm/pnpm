---
"@pnpm/store.cafs": patch
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Improve headless cold install performance (~35% faster in benchmarks) by reducing filesystem syscalls during package extraction and import.
