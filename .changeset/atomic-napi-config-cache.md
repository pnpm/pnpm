---
"@pnpm/napi": patch
---

Fixed a memory leak in the N-API bindings. Concurrent calls that resolved the same configuration each retained their own copy for the life of the process [pnpm/pnpm#14386](https://github.com/pnpm/pnpm/issues/14386).
