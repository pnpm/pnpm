---
"pacquet": patch
---

`pnpm install` no longer writes a package file through a symlink left at the path it is importing to. Copying such a file overwrote whatever the link pointed at. When the link pointed nowhere, the copy created that file. pnpm now leaves the link and the path it names alone, as it already did when hard linking or cloning.
