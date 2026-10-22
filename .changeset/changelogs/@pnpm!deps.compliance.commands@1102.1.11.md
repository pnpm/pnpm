## 1102.1.11

### Patch Changes

- `pnpm sbom` now reads project lockfiles in workspaces with `sharedWorkspaceLockfile: false`, including filtered and split output.

- `pnpm licenses list` reports no licenses for workspace projects not recorded in the lockfile. It errors when `pnpm-lock.yaml` is missing. Empty results now use the requested text or JSON format [pnpm/tasks#174](https://github.com/pnpm/tasks/issues/174).

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.3
  - @pnpm/cli.utils@1101.0.33
  - @pnpm/config.pick-registry-for-package@1101.0.3
  - @pnpm/config.reader@1102.3.6
  - @pnpm/config.version-policy@1100.2.9
  - @pnpm/config.writer@1100.0.34
  - @pnpm/deps.compliance.audit@1101.0.43
  - @pnpm/deps.compliance.license-scanner@1101.0.12
  - @pnpm/deps.compliance.sbom@1101.0.11
  - @pnpm/deps.security.signatures@1102.0.10
  - @pnpm/installing.commands@1101.4.7
  - @pnpm/lockfile.fs@1100.2.14
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/lockfile.utils@1102.1.7
  - @pnpm/lockfile.walker@1100.0.28
  - @pnpm/network.auth-header@1101.1.17
  - @pnpm/network.fetch@1100.1.23
  - @pnpm/pkg-manifest.utils@1100.4.10
  - @pnpm/store.path@1100.1.3
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.project-manifest-reader@1100.1.4
