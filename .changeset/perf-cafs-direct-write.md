---
"@pnpm/store.cafs": patch
"pnpm": patch
---

Write CAS files directly to their final content-addressed path instead of writing to a temp file and renaming. Uses `O_CREAT|O_EXCL` for safe concurrent multi-process writes. Eliminates ~30k rename syscalls per cold install.
