---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` hardlinks files from a group-writable or world-writable store again. Since store files take their group-write bit from the store directory, they did not match the install's umask, and pnpm copied every file into `node_modules` [#16677](https://github.com/pnpm/pnpm/issues/16677).
