---
"@pnpm/store.file-mode": patch
"pnpm": patch
---

Fixed `pnpm install` failing on macOS with `EISDIR` when it creates store directories in a group-writable store under a umask that removes owner read, such as `0o477`.
