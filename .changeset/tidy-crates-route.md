---
"@pnpm/pnpr": patch
---

Cargo package claims now cover both `-` and `_` spellings. Requests for a hosted crate cannot fall through to an upstream registry under the alternate spelling.
