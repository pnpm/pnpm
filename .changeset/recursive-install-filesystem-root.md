---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install` reported success without installing anything when the workspace projects' common ancestor was the filesystem root, such as `/` or a drive root like `C:\`. It now installs these projects [#16328](https://github.com/pnpm/pnpm/issues/16328).
