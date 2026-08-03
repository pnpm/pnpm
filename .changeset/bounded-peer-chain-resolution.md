---
"pacquet": patch
---

Installing a dependency chain whose packages carry peer dependencies no longer expands exponentially with the depth of the chain. A single project with a single such dependency could exhaust memory before finishing; it now resolves in tens of megabytes.
