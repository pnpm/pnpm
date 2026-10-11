---
"pacquet": patch
---

On macOS, `pnpm install` no longer fails with `ERR_PNPM_LOCKFILE_WRITE_FILE` when the project is on a virtiofs or SMB share. pnpm now falls back to a regular `fsync` when the file system rejects a full sync [#16846](https://github.com/pnpm/pnpm/issues/16846).
