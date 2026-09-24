---
"@pnpm/store.cafs": patch
"pnpm": patch
"pacquet": patch
---

When `pnpm install` repairs a store file that was modified through a hard link in `node_modules`, the repair now keeps the file's inode, so the hard-linked copies in other projects are healed at the same time [pnpm/pnpm#3445](https://github.com/pnpm/pnpm/issues/3445). Previously, only the project that ran the install got the restored content.
