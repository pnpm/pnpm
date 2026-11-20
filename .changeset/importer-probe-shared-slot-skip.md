---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Fixed `ERR_PNPM_LINKING_FAILED` on filesystems without copy-on-write reflinks when an install reuses an existing global virtual store entry before staging a package build. Also fixed merging `node_modules` directories during staged package imports on filesystems such as Btrfs that report `EEXIST` when renaming onto a non-empty directory.
