---
"@pnpm/cache.commands": minor
"pacquet": minor
"pnpm": minor
---

Added `pnpm cache path`, which prints the directory pnpm uses for its metadata cache. CI setups can use it to cache that directory — including the lockfile verification log, which lets a job skip re-checking an unchanged lockfile against the configured supply-chain policies.
