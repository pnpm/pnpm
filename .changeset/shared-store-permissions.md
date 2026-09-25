---
"pacquet": patch
"pnpm": patch
---

`pnpm install` keeps the owner, group, and mode of files already in a shared store, including `index.db`. New store files and directories inherit the store directory's group-write bit. When that directory is setgid, new files inherit its group. pnpm does not change a file's owner or group [pnpm/pnpm#12765](https://github.com/pnpm/pnpm/issues/12765).
