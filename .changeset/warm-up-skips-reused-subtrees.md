---
"pacquet": patch
"@pnpm/napi": patch
---

An install that resolves against an existing lockfile no longer reads registry metadata for the dependencies it takes from that lockfile. The speculative warm-up that prefetches packuments while the dependency tree is resolved now skips the subtrees the resolution reuses from the lockfile, which it never needed metadata for. On a warm store this removes thousands of metadata lookups from a repeat install of a large workspace.
