## 1100.3.18

### Patch Changes

- The progress output no longer overwrites the lines above it once it grows taller than the terminal window [#14270](https://github.com/pnpm/pnpm/issues/14270).

- The update notification now suggests `pnpm self-update` when `PNPM_HOME` manages the pnpm in use, and the [standalone install script](https://pnpm.io/installation) otherwise — under Corepack, or when another package manager installed pnpm. `pnpm self-update` under Corepack names the standalone install script too.

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.0
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.14
  - @pnpm/types@1102.1.0
