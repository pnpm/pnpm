## 1101.0.6

### Patch Changes

- Fixed `pnpm install` for workspace projects reached through a symlink, such as a `packages` directory that links to a folder outside the workspace. pnpm now installs their dependencies, and the links in their `node_modules` resolve [#1044](https://github.com/pnpm/pnpm/issues/1044).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/installing.modules-yaml@1101.0.4
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/types@1102.1.1
