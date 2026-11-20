---
"@pnpm/pnpr": patch
---

Cargo sparse index files now send ETags and return `304 Not Modified` when the client already has the current content.
