---
"@pnpm/network.auth-header": patch
"pnpm": patch
---

A `tokenHelper` command is now given a 60-second time limit. A helper that hangs (deadlock, stuck I/O) is killed and reported as an error instead of leaving the command waiting forever.
