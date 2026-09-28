---
"@pnpm/installing.commands": patch
"pnpm": patch
---

Fixed `pnpm install` reporting success without installing anything when the workspace projects' common ancestor is the filesystem root, such as `/` on POSIX or a drive root like `C:\` on Windows [#16328](https://github.com/pnpm/pnpm/issues/16328).
