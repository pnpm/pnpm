## 1100.4.0

### Minor Changes

- Added support for executing multiple scripts matching a RegExp passed to `pnpm run` (e.g., `pnpm run "/^build:.*/"`), running matched scripts in deterministic lexicographical order. Restored the `--sequential` (`-s`) CLI option for `pnpm run`, which forces `workspaceConcurrency` to 1 so that matched scripts run sequentially one by one across and within packages.

### Patch Changes

- Updated dependencies:
  - @pnpm/building.commands@1100.1.13
  - @pnpm/cli.utils@1101.0.16
  - @pnpm/config.reader@1101.12.2
  - @pnpm/deps.status@1100.1.9
  - @pnpm/engine.runtime.commands@1100.1.13
  - @pnpm/exec.lifecycle@1100.1.5
  - @pnpm/installing.client@1100.2.16
  - @pnpm/installing.commands@1100.10.7
  - @pnpm/workspace.injected-deps-syncer@1100.0.25
  - @pnpm/workspace.project-manifest-reader@1100.0.17
