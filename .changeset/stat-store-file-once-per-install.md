---
"pacquet": patch
---

`pnpm install` now reads a store file's permissions once per install, not once per link to it, so a file shared by many packages is checked once instead of once per package. A permission change through a hardlink makes the next import read the file again [#17500](https://github.com/pnpm/pnpm/issues/17500).
