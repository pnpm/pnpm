---
"pacquet": patch
---

`pnpm pack` now writes tarball entries grouped by file extension and file name, the order npm-packlist uses to shrink archives. Packages with many same-named files, such as project template collections, pack much smaller: create-vite-extra went from 823 KB to 89 KB. A workspace LICENSE file or a composed CHANGELOG.md is no longer appended at the end of the archive [#14766](https://github.com/pnpm/pnpm/issues/14766).
