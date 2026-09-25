---
"pacquet": patch
"pnpm": patch
---

`pnpm install` keeps the owner, group, and mode of files already in a shared store, including `index.db`. New store files and directories take the group-write bit and group of the store directory [pnpm/pnpm#12765](https://github.com/pnpm/pnpm/issues/12765).
