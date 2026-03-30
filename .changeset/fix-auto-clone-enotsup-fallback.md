---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Fixed a performance regression on Linux where `auto` import mode would copy every file instead of hardlinking on filesystems without reflink support (e.g. ext4).

The ENOTSUP fallback in `createClonePkg()` silently converted clone failures to copies, preventing the auto-importer from detecting that cloning is not supported and falling through to hardlinks. This caused a 2-9x slowdown on Linux CI for install operations.
