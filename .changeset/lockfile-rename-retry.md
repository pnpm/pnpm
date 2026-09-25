---
"@pnpm/fs.graceful-fs": patch
"@pnpm/lockfile.fs": patch
"pnpm": patch
"pacquet": patch
---

On Windows, pnpm now retries saving `pnpm-lock.yaml` for up to a minute while another process holds the file open. The save used to fail at once with `EPERM`, `EBUSY`, or "Access is denied" [#9461](https://github.com/pnpm/pnpm/issues/9461).
