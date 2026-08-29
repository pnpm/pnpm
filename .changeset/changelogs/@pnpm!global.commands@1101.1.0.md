## 1101.1.0

### Minor Changes

- `pnpm update -g` no longer downgrades a global package. `--latest` resolves the `latest` dist-tag, which can point at an older release than the one installed — after `pnpm add -g <pkg>@next`, for instance [#14270](https://github.com/pnpm/pnpm/issues/14270).

  `pnpm update -g` also no longer changes the pnpm version. pnpm's own global install belongs to `pnpm self-update` [#14270](https://github.com/pnpm/pnpm/issues/14270).

### Patch Changes

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.29
  - @pnpm/bins.remover@1100.0.22
  - @pnpm/bins.resolver@1100.0.16
  - @pnpm/cli.utils@1101.0.25
  - @pnpm/config.reader@1102.1.0
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/deps.inspection.list@1101.0.2
  - @pnpm/global.packages@1101.1.0
  - @pnpm/installing.deps-installer@1104.1.0
  - @pnpm/pkg-manifest.reader@1100.0.18
  - @pnpm/pkg-manifest.utils@1100.4.2
  - @pnpm/store.connection-manager@1101.1.0
  - @pnpm/types@1102.1.0
