---
"@pnpm/fs.indexed-pkg-importer": minor
"pnpm": minor
---

Added platform-specific file cloning (cloneDir) for faster package imports on CoW filesystems.

On macOS, uses cp -c for clonefile syscall support on APFS.
On Linux, uses reflink/CoW syscalls for Btrfs/XFS filesystems.
Falls back to regular copy when cloning is unavailable.
