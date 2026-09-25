---
"pacquet": patch
---

`pnpm pack` now prunes the directory that a `files` field exclusion names, so `files: ["**", "!**/test"]` ships without `test/`. It used to pack the excluded directory's contents, publishing whatever files the directory held [#15738](https://github.com/pnpm/pnpm/issues/15738).
