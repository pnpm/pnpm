---
"pacquet": patch
---

Numeric brace ranges such as `part{1..3}.js` now select the matching files when packing packages. Descending ranges, nonzero steps, and zero padding are supported in `files`, `.npmignore`, and `.gitignore` patterns.
