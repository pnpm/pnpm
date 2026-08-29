## 1101.1.0

### Minor Changes

- `pnpm update -g` no longer downgrades a global package. `--latest` resolves the `latest` dist-tag, which can point at an older release than the one installed — after `pnpm add -g <pkg>@next`, for instance [#14270](https://github.com/pnpm/pnpm/issues/14270).

  `pnpm update -g` also no longer changes the pnpm version. pnpm's own global install belongs to `pnpm self-update` [#14270](https://github.com/pnpm/pnpm/issues/14270).

### Patch Changes

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.16
  - @pnpm/crypto.hash@1100.0.3
  - @pnpm/pkg-manifest.reader@1100.0.18
  - @pnpm/types@1102.1.0
