---
"@pnpm/store.cafs": patch
"pnpm": patch
"pacquet": patch
---
When `pnpm install` repairs a store file that was modified through a hard link in `node_modules`, the repair now keeps the file's inode, so hard-linked copies in other projects are healed at the same time. Previously, only the project running the install received the restored content [pnpm/pnpm#3445](https://github.com/pnpm/pnpm/issues/3445).
