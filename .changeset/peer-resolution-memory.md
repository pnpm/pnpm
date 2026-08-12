---
"pnpm": patch
---

Reduced the memory the dependency resolver holds while resolving peer dependencies. Workspaces whose peer dependency graph contains many cycles realize millions of per-occurrence tree nodes, and each one was carrying more state than it needed.
