---
"pacquet": patch
---

Installing a workspace whose projects auto-install peer dependencies is substantially faster. Each round of the peer-hoist loop no longer scans the whole workspace once per project, so the cost of resolution grows with the workspace instead of with its square.
