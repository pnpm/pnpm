## 1101.0.1

### Patch Changes

- Batch workspace publishing accepts a shared scope-specific credential, rejects mismatched credentials for a registry before publishing, and runs the `publish` and `postpublish` scripts after each completed registry group [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).

- Updated dependencies:
  - @pnpm/config.reader@1102.0.1
  - @pnpm/engine.runtime.commands@1101.0.1
  - @pnpm/engine.runtime.node-resolver@1101.2.2
  - @pnpm/installing.client@1100.3.6
  - @pnpm/installing.commands@1101.0.1
  - @pnpm/lockfile.fs@1100.2.4
  - @pnpm/workspace.projects-filter@1100.0.39
