---
"@pnpm/pnpr": patch
---

pnpr writes a package's tombstone before it deletes anything, so a deletion that fails partway still keeps the name from other publishers. pnpr no longer restores an unpublished version when it finishes an interrupted publish at startup.
