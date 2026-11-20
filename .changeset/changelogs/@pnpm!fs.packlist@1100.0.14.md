## 1100.0.14

### Patch Changes

- `pnpm pack` and `pnpm publish` in a workspace now keep the workspace's ignore rules when a package's `.npmignore` is removed while pnpm lists the package's files.

- Installing an injected local package no longer fails with a "no such file or directory" error when another process renames or removes files in that package while pnpm lists them. This happened most often on macOS in workspaces with `sharedWorkspaceLockfile: false`, where the install of the package itself writes its lockfile at the same time.

- Updated dependencies:
  - @pnpm/workspace.project-manifest-reader@1100.1.7
