## 1100.2.11

### Patch Changes

- Fixed `pnpm add --save-exact`/`--save-prefix` and `pnpm update` writing a package's version with the `peerDependencies` range's prefix (e.g. `^19.2.7` instead of the requested `19.2.7`) whenever the same package also appeared in `peerDependencies`. A real `dependencies`/`devDependencies`/`optionalDependencies` entry now takes precedence over a same-named `peerDependencies` entry when computing the current specifiers [#13108](https://github.com/pnpm/pnpm/issues/13108).

- Updated dependencies:
  - @pnpm/core-loggers@1100.2.4
  - @pnpm/types@1101.5.0
