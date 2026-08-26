---
"pacquet": patch
---

Under `nodeLinker: isolated`, a Bit root-component member whose materialized copy carries no `package.json` now receives sibling symlinks for the dependencies its own lockfile snapshot declares, instead of a symlink to every other member of the root. The all-member fallback remains only when no snapshot exists.
