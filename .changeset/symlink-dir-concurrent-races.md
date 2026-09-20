---
"@pnpm/fs.symlink-dependency": patch
"pnpm": patch
---

Fixed `pnpm install` failing with `EEXIST` when a concurrent install cleared the file or directory that was occupying a symlink path. On Windows, a symlink another process is still holding is no longer moved aside and recreated.
