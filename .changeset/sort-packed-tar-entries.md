---
"pacquet": patch
---

`pnpm pack` writes tarball entries in npm-packlist's order, grouping them by file extension and file name. Packages that ship many same-named files, such as template collections, pack much smaller. A workspace LICENSE file or a composed CHANGELOG.md is no longer appended at the end of the archive [#14766](https://github.com/pnpm/pnpm/issues/14766).
