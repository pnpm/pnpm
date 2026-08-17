---
"@pnpm/store.commands": patch
"pnpm": patch
---

`pnpm store prune` no longer deletes the lockfile verification log. The log records which lockfile passed which supply-chain policies, so it stays valid across a prune of the store; keeping it lets the next install skip re-verifying an unchanged lockfile.
