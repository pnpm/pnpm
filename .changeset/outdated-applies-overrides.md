---
"pacquet": patch
---

`pnpm outdated` and `pnpm update --interactive` now apply `overrides` before looking up the latest version. A dependency overridden to an npm alias is compared with the alias target's versions, not with the versions of the package it replaces [#16719](https://github.com/pnpm/pnpm/issues/16719).
