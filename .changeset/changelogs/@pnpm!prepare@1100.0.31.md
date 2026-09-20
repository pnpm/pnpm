## 1100.0.31

### Patch Changes

- A signal sent to pnpm while it runs without a terminal, as a container runtime or a service manager does, now reaches the script even when the shell running it stays the script's parent. pnpm then waits for the script to finish shutting down. Such a signal used to end the shell at once or stay with it, and the script was never told to stop [#7374](https://github.com/pnpm/pnpm/issues/7374).

- Updated dependencies:
  - @pnpm/assert-project@1100.0.31
