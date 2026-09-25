---
"pacquet": patch
---

`pnpm add <pkg>@<version>` and `pnpm update <pkg>@<version>` now move the catalog entry onto the named version when the entry's range already covers it. For example, `^7.22.17` becomes `^7.29.6`, the same way `pnpm update <pkg>` moves an entry to the version it resolves [#13715](https://github.com/pnpm/pnpm/issues/13715).
