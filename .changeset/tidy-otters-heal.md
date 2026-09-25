---
"@pnpm/store.cafs": patch
"pnpm": patch
"pacquet": patch
---
When `pnpm install` repairs a store file that was modified through a hard link in `node_modules`, the repair now keeps the file's inode on Linux and macOS, so hard-linked copies in other projects are healed at the same time. Previously, only the project running the install received the restored content. On Windows the repair still replaces the file, so other projects are healed on their next install [pnpm/pnpm#3445](https://github.com/pnpm/pnpm/issues/3445).
