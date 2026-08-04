---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

Fixed the dependency status check wrongly reporting "up to date" when a `package.json`, `.pnpmfile.cjs`, or patch file was edited in the same second as the previous install, on filesystems that record mtimes at whole-second resolution (for example ext4 with 128-byte inodes). The optimistic repeat-install fast path and `verify-deps-before-run` compared mtimes strictly, so a same-second edit whose mtime rounded down looked unchanged and re-resolution was skipped. Such a file's whole second is now treated as possibly-modified, falling through to the content check; behavior on sub-second filesystems is unchanged.
