## 1102.2.2

### Patch Changes

- Install warnings no longer carry the text of a package's deprecation notice. The warning names the deprecated package and version, and the `pnpm:deprecation` event no longer carries the notice either. `pnpm view` still shows it on request.

  A deprecation warning now names the newest version of the package that is not deprecated, and says when reaching it means widening the range you declared:

  ```
  WARN  deprecated foo@1.0.0. 2.3.1 is not deprecated, outside the range you declared.
  ```

  pnpm works this out from the metadata it already fetched, so it costs no extra request. An install that reuses the lockfile without fetching metadata names no version.

  pnpm strips control characters from the package name and version in a deprecation warning, and from the notice `pnpm outdated --long` prints.

  The text sanitizer now also strips the Unicode line and paragraph separators U+2028 and U+2029.

- pnpm now deduplicates a package whose child dependency resolved an optional peer in one workspace project but not in another. Two copies of `next` could appear when only some projects could reach `styled-jsx`'s optional `babel-plugin-macros` peer [#14800](https://github.com/pnpm/pnpm/issues/14800).

- `pnpm install --prod` no longer downloads the registry packages that only a devDependency reaches [#881](https://github.com/pnpm/pnpm/issues/881).

- `pnpm update` and `pnpm audit --fix=update` no longer copy dependencies added by `packageExtensions`, a `readPackage` hook, or an override into `package.json`. Those dependencies keep the specifier the hook or override gives them. `pnpm update --latest` no longer resolves past that specifier. `pnpm audit --fix=update` now warns when one of them pins a vulnerable version. The warning points at `pnpm audit --fix` [#14928](https://github.com/pnpm/pnpm/issues/14928).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.0
  - @pnpm/deps.graph-hasher@1100.3.3
  - @pnpm/deps.path@1101.0.3
  - @pnpm/fetching.pick-fetcher@1100.1.11
  - @pnpm/fs.symlink-dependency@1100.0.20
  - @pnpm/hooks.types@1101.0.3
  - @pnpm/lockfile.preferred-versions@1100.0.34
  - @pnpm/lockfile.pruner@1100.0.24
  - @pnpm/lockfile.types@1100.1.2
  - @pnpm/lockfile.utils@1102.1.3
  - @pnpm/patching.config@1100.1.6
  - @pnpm/pkg-manifest.utils@1100.4.5
  - @pnpm/resolving.npm-resolver@1104.2.0
  - @pnpm/resolving.resolver-base@1101.3.0
  - @pnpm/store.controller-types@1101.3.0
