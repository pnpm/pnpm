## 1100.2.0

### Minor Changes

- Added `findWorkspaceDirSync`, `findPackagesSync`, `findWorkspaceProjectsSync`, `findWorkspaceProjectsNoCheckSync`, `readWorkspaceManifestSync`, and `readExactProjectManifestSync`.

### Patch Changes

- pnpm now treats a missing global `config.yaml`, `auth.ini`, or other optional config file as absent in Node.js-compatible runtimes such as StackBlitz WebContainers. Commands such as `pnpm --version` failed there with `ENOENT` [#14030](https://github.com/pnpm/pnpm/issues/14030).

- Updated dependencies:
  - @pnpm/error@1100.2.0
  - @pnpm/types@1102.1.1
