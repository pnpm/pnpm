## 1100.1.0

### Minor Changes

- Added `findWorkspaceDirSync`, `findPackagesSync`, `findWorkspaceProjectsSync`, `findWorkspaceProjectsNoCheckSync`, `readWorkspaceManifestSync`, and `readExactProjectManifestSync`.

### Patch Changes

- `pnpm add` and `pnpm install` keep an empty `peerDependencies`, `dependencies`, `devDependencies`, or `optionalDependencies` field that was already in `package.json`. pnpm still drops such a field when it removes the last entry itself, as `pnpm remove` does [#5096](https://github.com/pnpm/pnpm/issues/5096).

- Dependencies and executable binaries are now correctly linked and accessible for workspace packages using `publishConfig.directory` and `publishConfig.linkDirectory` [pnpm/pnpm#8338](https://github.com/pnpm/pnpm/issues/8338).

- Preserve CRLF line endings when modifying project manifests.

- Updated dependencies:
  - @pnpm/error@1100.2.0
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/pkg-manifest.utils@1100.4.6
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-writer@1100.0.18
