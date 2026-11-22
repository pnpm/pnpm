## 1101.0.17

### Patch Changes

- Plug'n'Play now combines custom export conditions from `NODE_OPTIONS` and command-line flags. It also respects Node.js's enabled `node-addons` and `module-sync` conditions.

- Fixed Plug'n'Play installs with local `file:` directory dependencies. Root projects can resolve their dependencies, and workspace projects with duplicate names retain their own dependencies.

- Fixed package maps resolving workspace and linked packages to the wrong directory when a path contains `#`, `?`, or `%`.

- Updated dependencies:
  - @pnpm/deps.path@1101.0.11
  - @pnpm/fs.symlink-dependency@1100.0.28
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/lockfile.utils@1102.1.13
  - @pnpm/types@1102.1.5
