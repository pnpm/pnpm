---
"@pnpm/pnpr": patch
---

The Cargo sparse-index discovery walk now keeps its parsed registry between waves instead of re-parsing every accumulated index file each wave. Stale index cache entries are deleted when they expire.
