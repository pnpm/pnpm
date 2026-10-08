---
"pacquet": patch
---

`pnpm outdated` and `pnpm update --interactive` now apply `overrides` before they look up the latest version. Before, a dependency overridden to an npm alias was compared with the latest version of the package the override replaces [#16719](https://github.com/pnpm/pnpm/issues/16719).
