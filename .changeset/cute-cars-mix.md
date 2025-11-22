---
"@pnpm/fs.hard-link-dir": patch
"pnpm": patch
---

Handle ENOENT errors thrown by `fs.linkSync()`, which can occur in containerized environments (OverlayFS) instead of EXDEV. The operation now gracefully falls back to `fs.copyFileSync()` in these cases [#10217](https://github.com/pnpm/pnpm/issues/10217).
