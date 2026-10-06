## 1101.2.8

### Patch Changes

- `pnpm publish` now rejects manifests and README files larger than 64 MiB in pre-built tarballs before reading them into memory.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.32
  - @pnpm/config.reader@1102.3.4
  - @pnpm/constants@1102.0.1
  - @pnpm/deps.path@1101.0.6
  - @pnpm/engine.runtime.commands@1101.1.8
  - @pnpm/engine.runtime.node-resolver@1101.3.7
  - @pnpm/error@1100.2.2
  - @pnpm/exec.lifecycle@1100.1.23
  - @pnpm/fetching.directory-fetcher@1100.0.38
  - @pnpm/fs.indexed-pkg-importer@1100.0.33
  - @pnpm/fs.packlist@1100.0.8
  - @pnpm/installing.client@1100.3.15
  - @pnpm/installing.commands@1101.4.5
  - @pnpm/lockfile.fs@1100.2.13
  - @pnpm/lockfile.peer-edges@1100.0.2
  - @pnpm/network.auth-header@1101.1.16
  - @pnpm/network.fetch@1100.1.22
  - @pnpm/network.web-auth@1101.6.3
  - @pnpm/releasing.exportable-manifest@1100.3.7
  - @pnpm/releasing.versioning@1100.3.6
  - @pnpm/resolving.npm-resolver@1104.2.5
  - @pnpm/workspace.projects-filter@1100.0.48
  - @pnpm/workspace.projects-graph@1100.0.43
  - @pnpm/workspace.task-scheduler@1100.0.4
  - @pnpm/workspace.workspace-manifest-writer@1100.2.6
