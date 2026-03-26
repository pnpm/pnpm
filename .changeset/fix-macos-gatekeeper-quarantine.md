---
"@pnpm/fs.indexed-pkg-importer": patch
---

Fix macOS Gatekeeper blocking native `.node` binaries by removing quarantine xattr after file copy.

When pnpm copies files from its content-addressable store to `node_modules`, macOS preserves the `com.apple.quarantine` extended attribute. If this xattr is present on store blobs (e.g., from packages installed under Gatekeeper-enabled apps like Git clients), native binaries trigger Gatekeeper dialogs blocking execution even though pnpm has verified file integrity.

This change removes the `com.apple.quarantine` xattr after file operations (copy, reflink/clone), matching Homebrew's approach for verified downloads. The fix is macOS-only, non-fatal (logs warnings on errors), and only removes quarantine while preserving other extended attributes.

Fixes #11056
