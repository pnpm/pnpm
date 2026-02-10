---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Fixed a race condition when multiple worker threads import the same package to the global virtual store concurrently. The rename operation now tolerates `ENOTEMPTY`/`EEXIST` errors if another thread already completed the import.
