## 1100.3.22

### Patch Changes

- With the default and append-only reporters, installs with `--loglevel warn` or `--loglevel error` now print the full output of a failed install script. The output of successful scripts, including the root project's own install hooks, stays hidden. With `--loglevel warn`, pnpm also prints ignored build script warnings.

- Updated dependencies:
  - @pnpm/error@1100.2.1
