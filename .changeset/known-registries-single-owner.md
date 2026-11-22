---
"pacquet": patch
---

Fixed the order in which pnpm matches a lockfile's recorded tarball URL against known registry URLs. Two registry URLs of equal length were previously ordered arbitrarily, so which one a tarball URL matched could differ between runs.
