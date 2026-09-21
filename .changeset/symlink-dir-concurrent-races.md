---
"@pnpm/bins.linker": patch
"@pnpm/engine.pm.commands": patch
"@pnpm/exec.commands": patch
"@pnpm/fs.symlink-dependency": patch
"@pnpm/global.commands": patch
"@pnpm/installing.env-installer": patch
"@pnpm/installing.linking.hoist": patch
"@pnpm/store.controller": patch
"pnpm": patch
---

Fixed `pnpm install` failing with `EEXIST` when a concurrent install cleared the file or directory that was occupying a symlink path. On Windows, a symlink another process is still holding is no longer moved aside and recreated.
