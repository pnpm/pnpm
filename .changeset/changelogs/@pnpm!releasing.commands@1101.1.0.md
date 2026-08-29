## 1101.1.0

### Minor Changes

- `pnpm stage approve` now approves several staged packages at once. Run it without a stage id to pick from the staged versions interactively, or pass a list of stage ids. The whole batch is approved with a single one-time password, and pnpm asks for a new one only once the registry stops accepting it. Inside a workspace, the selected packages are approved in dependency order, and a package whose workspace dependency could not be approved is skipped instead of being published against a dependency that never reached the registry.

### Patch Changes

- Fixed `pnpm deploy --prod` failing when an excluded dev dependency was also declared as an optional peer dependency [pnpm/pnpm#14302](https://github.com/pnpm/pnpm/issues/14302).

- Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.16
  - @pnpm/cli.utils@1101.0.25
  - @pnpm/config.pick-registry-for-package@1101.0.1
  - @pnpm/config.reader@1102.1.0
  - @pnpm/deps.graph-sequencer@1101.0.0
  - @pnpm/deps.path@1101.0.1
  - @pnpm/engine.runtime.commands@1101.1.0
  - @pnpm/engine.runtime.node-resolver@1101.2.3
  - @pnpm/exec.lifecycle@1100.1.15
  - @pnpm/fetching.directory-fetcher@1100.0.31
  - @pnpm/fs.indexed-pkg-importer@1100.0.27
  - @pnpm/installing.client@1100.3.7
  - @pnpm/installing.commands@1101.1.0
  - @pnpm/lockfile.fs@1100.2.5
  - @pnpm/lockfile.types@1100.1.0
  - @pnpm/network.auth-header@1101.1.12
  - @pnpm/network.fetch@1100.1.14
  - @pnpm/network.web-auth@1101.5.0
  - @pnpm/releasing.exportable-manifest@1100.2.5
  - @pnpm/releasing.versioning@1100.2.7
  - @pnpm/resolving.npm-resolver@1104.1.0
  - @pnpm/resolving.registry.types@1100.2.0
  - @pnpm/resolving.resolver-base@1101.2.0
  - @pnpm/types@1102.1.0
  - @pnpm/workspace.projects-filter@1100.0.40
  - @pnpm/workspace.projects-graph@1100.0.35
  - @pnpm/workspace.projects-reader@1101.0.25
  - @pnpm/workspace.projects-sorter@1101.0.0
  - @pnpm/workspace.workspace-manifest-writer@1100.1.2
