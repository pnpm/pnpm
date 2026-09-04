---
"@pnpm/store.cafs": patch
"pnpm": patch
"pacquet": patch
---

Fixed concurrent installs that share a store occasionally failing with an ENOENT error while importing a package file. Store integrity verification no longer deletes a mismatched or unreadable file, because another install may be importing from it at that moment. The re-fetch replaces the file atomically instead [#14353](https://github.com/pnpm/pnpm/issues/14353).
