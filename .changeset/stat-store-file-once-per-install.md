---
"pacquet": patch
---

`pnpm install` now reads a store file's permissions once per install, not once per link to it. On the 67,068-file install in the report, that removed 66,898 `statx` calls [#17500](https://github.com/pnpm/pnpm/issues/17500).
