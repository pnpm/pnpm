---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Fix macOS Gatekeeper blocking native binaries (`.node`, `.dylib`, `.so`) by removing the `com.apple.quarantine` extended attribute after importing them from the store.

When pnpm imports files from its content-addressable store into `node_modules`, macOS preserves extended attributes, including `com.apple.quarantine`. If this xattr is present on a store blob (e.g. it was first written under a Gatekeeper-enabled app such as a Git client), it propagates to `node_modules`, and Gatekeeper blocks the native binary from loading even though pnpm already verified the file's integrity against the lockfile.

After importing a package, pnpm now strips `com.apple.quarantine` from its native binaries, matching Homebrew's behaviour of dropping quarantine from verified downloads. The cleanup is macOS-only, runs in a single batched `xattr` call per package, is restricted to native binaries (other files are untouched), and is non-fatal (it logs a warning on unexpected errors).

Fixes #11056
