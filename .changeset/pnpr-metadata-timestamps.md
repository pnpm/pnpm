---
"@pnpm/pnpr": patch
---

pnpr now uses one clock for publication retention and archive reclamation. This changes the metadata schema. Existing alpha databases must be reset as described in the storage documentation.
