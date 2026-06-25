---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm pack-app` now rejects `--entry` / `pnpm.app.entry` and `--output-dir` / `pnpm.app.outputDir` values that are absolute paths or escape the project directory via `..` (or a symlink that resolves outside it), and refuses to write the produced executable when its target path already exists as a symlink (or other non-regular file). This prevents a repository-controlled `package.json` from embedding host files (such as an SSH key) into the produced executable, writing build artifacts outside the project, or overwriting an arbitrary file through a committed symlink. The new error codes are `ERR_PNPM_PACK_APP_ENTRY_OUTSIDE_PROJECT`, `ERR_PNPM_PACK_APP_OUTPUT_DIR_OUTSIDE_PROJECT`, and `ERR_PNPM_PACK_APP_OUTPUT_FILE_NOT_REGULAR`.
