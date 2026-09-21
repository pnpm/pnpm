---
"@pnpm/workspace.root-finder": patch
"@pnpm/workspace.projects-reader": patch
"@pnpm/workspace.project-manifest-reader": patch
"@pnpm/workspace.workspace-manifest-reader": patch
"pnpm": patch
---

`findWorkspaceDirSync`, `findPackagesSync`, and `findWorkspaceProjectsSync` are now exported for synchronous workspace operations.
