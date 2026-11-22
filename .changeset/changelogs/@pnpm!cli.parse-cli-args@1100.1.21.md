## 1100.1.21

### Patch Changes

- Global commands (`--global`, `-g`) no longer load `.pnpmfile.cjs`, `pnpm-workspace.yaml`, or `.npmrc` from the current working directory. `pnpm why -g` now inspects globally installed packages.

- An unknown option passed via a shorthand is now reported as typed, and the shorthand of the other case is suggested when the command supports it. For instance, `pnpm update -l` used to fail with `Unknown option: 'long'`, as `-l` is short for `--long`. Now it fails with `Unknown option: '-l'` and suggests `-L` (`--latest`).

- Updated dependencies:
  - @pnpm/error@1100.2.3
  - @pnpm/workspace.root-finder@1100.1.8
