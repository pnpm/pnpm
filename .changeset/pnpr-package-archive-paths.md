---
"@pnpm/pnpr": patch
---

pnpr rejects Cargo and Python uploads with ambiguous archive paths. Python ZIP uploads now have the same 512 MiB decompressed size limit as compressed tar uploads.
