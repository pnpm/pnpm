---
"@pnpm/workspace.read-manifest": minor
---

The type definition for the `packages` field of the `WorkspaceManifest` is now non-null. The `readWorkspaceManifest` function expects this field to be present and throws an error otherwise.
