---
"pacquet": patch
---

Resolving peer dependencies in a workspace whose dependency graph contains many peer-dependency cycles now needs less than half the memory and finishes about twice as fast. Verdicts computed inside dependency cycles are now cached and reused for the occurrences they are provably valid for, instead of being recomputed for every occurrence.
