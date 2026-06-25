---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm pack-app` now rejects `--entry` / `pnpm.app.entry` and `--output-dir` / `pnpm.app.outputDir` values that are absolute paths or escape the project directory via `..` (or a symlink that resolves outside it). This prevents a repository-controlled `package.json` from embedding host files (such as an SSH key) into the produced executable or writing build artifacts outside the project. The new error codes are `ERR_PNPM_PACK_APP_ENTRY_OUTSIDE_PROJECT` and `ERR_PNPM_PACK_APP_OUTPUT_DIR_OUTSIDE_PROJECT`.
