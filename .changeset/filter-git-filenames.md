---
"pacquet": patch
---

`--filter "[<since>]"` now detects changes in projects whose directory names contain non-ASCII characters. The change used to be credited to the parent project.

`changedFilesIgnorePattern` and `testPattern` now match changed files whose names contain non-ASCII characters.
