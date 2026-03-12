---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Fixed intermittent failures when multiple `pnpm dlx` calls run concurrently for the same package. When the global virtual store is enabled, the importer now skips instead of doing a swap-rename when the target directory already exists, since GVS paths are content-addressed and existing targets always have the correct content.
