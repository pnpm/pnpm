---
"@pnpm/pnpr": patch
---

pnpr continues resolving requests after serving more than 1,024 distinct client configurations. Inactive configurations are reclaimed when the cache is full.

Excess concurrent npm resolution and verification requests return `503 Service Unavailable` until active work finishes.
