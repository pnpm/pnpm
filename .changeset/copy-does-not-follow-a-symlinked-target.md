---
"pacquet": patch
---

`pnpm install` no longer writes a package file through a symlink left at the path it is importing to. Copying such a file overwrote whatever the link pointed at, and created the file when the link pointed nowhere. pnpm now reports the occupied path, as it already did when hard linking or cloning.
