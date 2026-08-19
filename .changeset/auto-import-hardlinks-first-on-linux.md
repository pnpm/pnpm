---
"@pnpm/fs.indexed-pkg-importer": minor
"pacquet": minor
"pnpm": minor
---

`packageImportMethod: auto` now tries hardlinks before cloning on Linux. A reflink materializes a new inode and copies extent bookkeeping inside the filesystem's metadata trees, where a hardlink is one directory entry — on btrfs this roughly halves the time an install spends materializing `node_modules` from a warm store. ext4 installs are unchanged (cloning was never supported there, so `auto` already hardlinked), and macOS keeps clone-first, where APFS `clonefile` is the platform's cheap primitive. Cloning remains the fallback when the store refuses hardlinks, and remains available explicitly via `packageImportMethod: clone`.
