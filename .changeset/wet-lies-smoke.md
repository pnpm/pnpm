---
"@pnpm/default-reporter": patch
pnpm: patch
---

Fix a bug causing errors to be printed as `Cannot read properties of undefined (reading 'code')` instead of the underlying reason when using the pnpm store server.
