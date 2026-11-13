## 1101.4.11

### Patch Changes

- When `pnpm install` adds placeholder entries to `allowBuilds` in `pnpm-workspace.yaml`, it no longer copies `allowBuilds` entries from other config sources into the file.

- `pnpm fetch` no longer fails with `ENOENT` when `nodeLinker` is `hoisted` and the project configures `patchedDependencies` or `allowBuilds`. `pnpm fetch` also preserves an existing hoisted `node_modules` directory [#18037](https://github.com/pnpm/pnpm/issues/18037).

- `pnpm install` now checks the project's `engines` field when the project is already up to date. A repeat install of a project outside a workspace warns about an incompatible Node.js version, or fails with `engineStrict` enabled [pnpm/pnpm#17751](https://github.com/pnpm/pnpm/issues/17751).

- A repeat `pnpm install` with `nodeLinker: pnp` now applies dependency and configuration changes. Before, it printed "Already up to date" and left the lockfile unchanged.

- A workspace project that downloads its own Node.js through `devEngines.runtime` now has its `engines.node` checked against that Node.js. Before, pnpm checked it against the shared Node.js version, so `engineStrict` could reject a project whose scripts run on a compatible runtime [pnpm/tasks#300](https://github.com/pnpm/tasks/issues/300).

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.16
  - @pnpm/building.policy@1100.1.10
  - @pnpm/cli.utils@1101.0.35
  - @pnpm/config.reader@1102.3.9
  - @pnpm/config.writer@1100.0.37
  - @pnpm/deps.github-actions@1100.1.19
  - @pnpm/deps.inspection.outdated@1100.1.42
  - @pnpm/deps.path@1101.0.9
  - @pnpm/deps.security.signatures@1102.0.12
  - @pnpm/deps.status@1100.1.35
  - @pnpm/fs.graceful-fs@1100.2.6
  - @pnpm/global.commands@1102.0.13
  - @pnpm/global.packages@1101.1.11
  - @pnpm/hooks.pnpmfile@1100.0.39
  - @pnpm/installing.context@1101.0.16
  - @pnpm/installing.dedupe.check@1100.1.19
  - @pnpm/installing.deps-installer@1104.2.10
  - @pnpm/installing.deps-resolver@1102.2.13
  - @pnpm/installing.env-installer@1103.0.16
  - @pnpm/installing.modules-yaml@1101.0.9
  - @pnpm/lockfile.fs@1100.2.18
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/network.fetch@1100.1.25
  - @pnpm/pkg-manifest.utils@1100.4.12
  - @pnpm/resolving.local-resolver@1101.2.9
  - @pnpm/resolving.npm-resolver@1104.2.10
  - @pnpm/resolving.resolver-base@1101.3.5
  - @pnpm/store.connection-manager@1101.3.9
  - @pnpm/store.controller@1102.2.10
  - @pnpm/workspace.injected-deps-syncer@1100.0.49
  - @pnpm/workspace.project-manifest-reader@1100.1.6
  - @pnpm/workspace.projects-filter@1100.0.54
  - @pnpm/workspace.projects-graph@1100.0.48
  - @pnpm/workspace.projects-reader@1101.1.7
  - @pnpm/workspace.state@1100.0.55
  - @pnpm/workspace.workspace-manifest-writer@1100.2.11
