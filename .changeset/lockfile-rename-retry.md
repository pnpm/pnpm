---
"@pnpm/fs.graceful-fs": patch
"@pnpm/lockfile.fs": patch
"pnpm": patch
"pacquet": patch
---

On Windows, pnpm now waits briefly when another process holds `pnpm-lock.yaml` open while pnpm saves it. The save used to fail at once with `EPERM`, `EBUSY`, or "Access is denied" [#9461](https://github.com/pnpm/pnpm/issues/9461).
