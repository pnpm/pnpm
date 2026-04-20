---
"@pnpm/fetching.directory-fetcher": patch
"pnpm": patch
---

Fix installing a directory dependency (`file:<dir>`) from an absolute path on a different drive on Windows. The directory fetcher was joining the stored directory onto `lockfileDir`, which on Windows concatenates an absolute cross-drive path literally (`path.join('D:\\...', 'C:\\Users\\...')` → `'D:\\...\\C:\\Users\\...'`). Use `path.resolve` so absolute paths are respected. This surfaced as an ENOENT during `pnpm setup` in CI when `PNPM_HOME` and the OS temp directory were on different drives.
