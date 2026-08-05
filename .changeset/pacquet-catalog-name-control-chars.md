---
"pacquet": patch
---

A catalog name containing a control character no longer corrupts `pnpm-workspace.yaml`. `pnpm add --save-catalog-name "$(printf 'a\nb')"` (or the same value in `saveCatalogName`) now fails with `ERR_PNPM_WORKSPACE_MANIFEST_WRITER_INVALID_CONTROL_CHARACTER` and leaves the file untouched, matching how the writer already treats `allowBuilds` and `overrides` entries.
