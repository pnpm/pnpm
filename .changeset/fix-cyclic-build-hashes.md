---
"@pnpm/deps.graph-hasher": patch
"pacquet": patch
"pnpm": patch
---

Fixed global virtual store hashes for dependency cycles. Every package that transitively depends on an allowed build now includes the engine in its store path, independent of traversal order [pnpm/pnpm#14341](https://github.com/pnpm/pnpm/issues/14341).
