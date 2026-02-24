---
"@pnpm/workspace.state": patch
"pnpm": patch
---

Fixed `optimisticRepeatInstall` skipping install when `overrides`, `packageExtensions`, `ignoredOptionalDependencies`, `patchedDependencies`, or `peersSuffixMaxLength` changed.
