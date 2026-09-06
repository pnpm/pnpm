---
"pacquet": patch
---

Fixed `pnpm setup` failing with `ERR_PNPM_DIRECTORY_FETCHER_PATH_ESCAPE` on Windows [#14618](https://github.com/pnpm/pnpm/issues/14618). A `file:` dependency whose directory is a symlink or junction is now packed from the directory it points at. Files that a symlink inside the package reaches outside that directory are still refused.
