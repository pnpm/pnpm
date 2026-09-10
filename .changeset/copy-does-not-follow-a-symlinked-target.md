---
"pacquet": patch
---

`pnpm install` no longer writes a package file through a symlink left at the path it is importing to. Copying such a file overwrote whatever the link pointed at. When the link pointed nowhere, the copy created that file. An executable package file also made the link's target executable.
