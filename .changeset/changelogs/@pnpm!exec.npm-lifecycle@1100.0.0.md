## 1100.0.0

### Patch Changes

- `pnpm exec` and `pnpm dlx` now wait for the command to finish shutting down after `Ctrl+C`. A signal sent to pnpm alone now reaches the command, the way it does with `pnpm run`. pnpm used to exit on the interrupt and terminate the command while it was still shutting down [#7374](https://github.com/pnpm/pnpm/issues/7374).

- The `@pnpm/npm-lifecycle` package is now available as `@pnpm/exec.npm-lifecycle`.

- A signal sent to pnpm while it runs without a terminal, as a container runtime or a service manager does, now reaches the script even when the shell running it stays the script's parent. pnpm then waits for the script to finish shutting down. Such a signal used to end the shell at once or stay with it, and the script was never told to stop [#7374](https://github.com/pnpm/pnpm/issues/7374).
