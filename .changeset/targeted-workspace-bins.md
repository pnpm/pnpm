---
"pacquet": patch
---

Workspace installs are substantially faster (~0.7 s on a 60-project workspace): after hoisting, pnpm now shims only the bins of publicly hoisted workspace packages instead of re-walking every project's `node_modules` to rediscover bins that were already linked.
