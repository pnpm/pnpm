---
"@pnpm/workspace.workspace-manifest-reader": patch
"pnpm": patch
---

Reject `null` named catalogs in workspace manifests with `InvalidWorkspaceManifestError` instead of crashing with a raw `TypeError`.
