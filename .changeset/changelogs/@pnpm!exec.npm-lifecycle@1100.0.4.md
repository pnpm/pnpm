## 1100.0.4

### Patch Changes

- Scripts run without a terminal no longer start a second `sh` each. One watchdog per pnpm command now ends every script's process group if pnpm is killed, so `pnpm -r run` across many projects starts half as many processes [#16489](https://github.com/pnpm/pnpm/issues/16489).

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.2
