## 1100.1.4

### Patch Changes

- `pnpm install` now fails with `ERR_PNPM_IGNORED_BUILDS` on a repeat install when `strictDepBuilds` is on and a dependency's build is still undecided. A repeat install against an existing `node_modules` reported success where a fresh install failed [pnpm/pnpm#10450](https://github.com/pnpm/pnpm/issues/10450).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.5
  - @pnpm/deps.path@1101.0.5
