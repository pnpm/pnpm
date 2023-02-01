---
"@pnpm/plugin-commands-audit": patch
---

Fix AUDIT_TABLE_OPTIONS not to overwrite TABLE_OPTIONS, which prevents breaking other table outputs such like `pnpm outdated` [#6017](https://github.com/pnpm/pnpm/issues/6017).
