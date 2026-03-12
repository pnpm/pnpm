---
"@pnpm/fs.indexed-pkg-importer": patch
"@pnpm/plugin-commands-script-runners": patch
"@pnpm/core": patch
"@pnpm/link-bins": patch
"pnpm": patch
---

Fixed intermittent failures when multiple `pnpm dlx` calls run concurrently for the same package. When the global virtual store is enabled, the importer now verifies file content before skipping a rename, avoiding destructive swap-renames that break concurrent processes. Also tolerates EPERM during bin creation on Windows and properly propagates `enableGlobalVirtualStore` through the install pipeline.
