---
"@pnpm/workspace.injected-deps-syncer": patch
"pnpm": patch
---

Sync bin links after injected dependencies are updated by build scripts. This ensures that binaries created during build processes are properly linked and accessible to consuming projects [#10057](https://github.com/pnpm/pnpm/issues/10057).
