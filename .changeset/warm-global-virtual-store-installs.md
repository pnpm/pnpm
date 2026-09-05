---
"pacquet": patch
---

Sped up installs that use the global virtual store. A slot that is already materialized no longer runs the store-integrity pass or the link pass, the dependency-graph hashes are derived without intermediate JSON values, and the derived slot-path map is cached in the cache directory instead of being recomputed on every install.
