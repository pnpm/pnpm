---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm pack-app` now accepts an entry file or output directory inside the project whose name starts with two dots, such as `..build/entry.cjs`. It used to fail with `ERR_PNPM_PACK_APP_ENTRY_OUTSIDE_PROJECT`.
