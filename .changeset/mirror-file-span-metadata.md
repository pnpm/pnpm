---
"pacquet": patch
---

Reduced peak install memory: cached registry metadata is now read on demand from the on-disk metadata cache instead of being held in memory for the whole resolution. Resolving a large peer-heavy graph (`@teambit/bit`) peaks at about 1.3 GB instead of 3.2 GB, and a full cold install of it stays under 2 GB [#13681](https://github.com/pnpm/pnpm/issues/13681).
