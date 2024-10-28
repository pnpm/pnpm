---
"@pnpm/constants": major
"pnpm": major
---

Store version bumped to v10. The new store layout has a different directory called "index" for storing the package content mappings. Previously these files were stored in the same directory where the package contents are (in "files"). The new store has also a new format for storing the mappings for side-effects cache.
