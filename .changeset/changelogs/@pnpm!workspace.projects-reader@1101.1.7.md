## 1101.1.7

### Patch Changes

- A workspace project that downloads its own Node.js through `devEngines.runtime` now has its `engines.node` checked against that Node.js. Before, pnpm checked it against the shared Node.js version, so `engineStrict` could reject a project whose scripts run on a compatible runtime [pnpm/tasks#300](https://github.com/pnpm/tasks/issues/300).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.35
  - @pnpm/pkg-manifest.utils@1100.4.12
  - @pnpm/workspace.project-manifest-reader@1100.1.6
