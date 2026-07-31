## 1100.3.14

### Patch Changes

- The default reporter no longer depends on `@pnpm/config.reader` at runtime: it declares its own minimal `ReporterPnpmConfig` type for the config fields it reads. Hosts that embed the reporter no longer pull in the config-reader dependency tree.

- Updated dependencies:
  - @pnpm/cli.meta@1100.0.13
  - @pnpm/core-loggers@1100.3.1
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.11
  - @pnpm/types@1101.8.0
