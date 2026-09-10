---
"pacquet": patch
---

`pnpm install` now copies a package file whose store entry has reached the filesystem's limit on hard links to one file. NTFS allows 1024 and ext4 allows 65000, and a store shared by enough projects reaches the NTFS limit. Such a file failed the install under `packageImportMethod: hardlink`. Under `auto` it stopped pnpm hard linking for the rest of the install.
