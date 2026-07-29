---
"pacquet": patch
---

Fixed quadratic time and memory use when resolving a large multi-project workspace from scratch. Resolving a workspace with hundreds of projects sharing thousands of packages previously took minutes and several gigabytes of memory; it now completes in seconds.
