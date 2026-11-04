---
"@pnpm/pnpr": patch
---

pnpr no longer frees a package name when a package deletion fails partway. pnpr no longer restores an unpublished version when it finishes an interrupted publish at startup.
