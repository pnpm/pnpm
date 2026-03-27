---
"@pnpm/store.cafs": patch
"pnpm": patch
---

Fix a bug where the CAS locker cache was not updated when a file already existed with correct integrity, causing repeated integrity re-verification on subsequent lookups within the same process.
