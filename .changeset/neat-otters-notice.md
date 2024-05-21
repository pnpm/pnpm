---
"@pnpm/workspace.find-packages": minor
---

The `findWorkspacePackages` and `findWorkspacePackagesNoCheck` functions now accept a `workspaceManifest` field in its options object. If provided, this workspace manifest will be used and `pnpm-workspace.yaml` will not be read directly.
