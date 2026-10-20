---
"pacquet": patch
---

Brace ranges such as `part{1..3}.js` and `part{a..c}.js` now select the matching files when packing packages. Descending ranges, steps, and zero padding are supported in `files`, `.npmignore`, and `.gitignore` patterns. A zero step uses the default increment of 1.
