## 1100.0.36

### Patch Changes

- Fixed frozen installs replacing a hoisted dependency with a workspace package of the same name. A later `pnpm dedupe` then removed the hoisted link [#16485](https://github.com/pnpm/pnpm/issues/16485).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.36
  - @pnpm/config.matcher@1100.0.3
  - @pnpm/core-loggers@1101.0.2
  - @pnpm/fs.symlink-dependency@1100.0.23
