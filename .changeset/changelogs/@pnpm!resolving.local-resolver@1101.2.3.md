## 1101.2.3

### Patch Changes

- Installing or adding dependencies no longer fails when a previously installed local tarball file was deleted from disk [#8367](https://github.com/pnpm/pnpm/issues/8367).

- `pnpm add` and `pnpm install` now support installing bzip2 compressed tarballs [https://github.com/pnpm/pnpm/issues/6761](https://github.com/pnpm/pnpm/issues/6761).

- `pnpm install` now resolves local tarballs specified with bare UNC paths on Windows [pnpm/pnpm#1669](https://github.com/pnpm/pnpm/issues/1669).

- `pnpm update --global` now skips a global package installed from a `file:` path that no longer exists, prints a warning, and updates the remaining global packages. Previously the whole update failed with `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND` [#12533](https://github.com/pnpm/pnpm/issues/12533).

- Dependencies and executable binaries are now correctly linked and accessible for workspace packages using `publishConfig.directory` and `publishConfig.linkDirectory` [pnpm/pnpm#8338](https://github.com/pnpm/pnpm/issues/8338).

- pnpm now recognizes local paths with forward slashes on Windows.

- On Windows, `pnpm add` and `pnpm update` now write relative `file:` and `link:` specifiers with forward slashes to `package.json` and the lockfile. They used to write backslashes, so the same project produced different files on Windows and on other systems [#7497](https://github.com/pnpm/pnpm/issues/7497), [#9687](https://github.com/pnpm/pnpm/issues/9687).

- `pnpm install` now reads the same local tarball it installs when a dependency's absolute `file:` path contains `..`. Such a path could install a different tarball than the one it read, failing with `ERR_PNPM_TARBALL_INTEGRITY`, or fail to resolve at all.

- `packageExtensions` and `overrides` entries with a ranged selector (such as `@<X` or `@*`) no longer match a local directory dependency that has no `package.json` [pnpm/pnpm#15007](https://github.com/pnpm/pnpm/issues/15007).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.6
  - @pnpm/error@1100.2.0
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
