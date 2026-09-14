## 1102.0.1

### Patch Changes

- Fixed `pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` mutating global bins or install directories after only partially reading an installed package group. If any declared package manifest is missing, malformed, or unreadable, pnpm now fails before activation or removal and leaves the existing global installation intact [pnpm/pnpm#13796](https://github.com/pnpm/pnpm/issues/13796).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.31
  - @pnpm/cli.utils@1101.0.27
  - @pnpm/config.reader@1102.2.0
  - @pnpm/deps.inspection.list@1101.0.4
  - @pnpm/global.packages@1101.1.2
  - @pnpm/installing.deps-installer@1104.1.2
  - @pnpm/pkg-manifest.utils@1100.4.4
  - @pnpm/store.connection-manager@1101.1.2
