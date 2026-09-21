## 1102.1.15

### Patch Changes

- Install warnings no longer carry the text of a package's deprecation notice. The warning names the deprecated package and version, and the `pnpm:deprecation` event no longer carries the notice either. `pnpm view` still shows it on request.

  A deprecation warning now names the newest version of the package that is not deprecated, and says when reaching it means widening the range you declared:

  ```
  WARN  deprecated foo@1.0.0. 2.3.1 is not deprecated, outside the range you declared.
  ```

  pnpm works this out from the metadata it already fetched, so it costs no extra request. An install that reuses the lockfile without fetching metadata names no version.

  pnpm strips control characters from the package name and version in a deprecation warning, and from the notice `pnpm outdated --long` prints.

  The text sanitizer now also strips the Unicode line and paragraph separators U+2028 and U+2029.

- `pnpm install` now installs git-hosted dependencies without preparing them when their builds are explicitly denied by `allowBuilds`. Dependencies that require preparation still need an explicit allow or deny decision [pnpm/pnpm#10522](https://github.com/pnpm/pnpm/issues/10522).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.1.7
  - @pnpm/core-loggers@1101.0.0
  - @pnpm/deps.path@1101.0.3
  - @pnpm/exec.prepare-package@1100.0.37
  - @pnpm/fetching.fetcher-base@1100.2.10
  - @pnpm/fetching.pick-fetcher@1100.1.11
  - @pnpm/fs.graceful-fs@1100.2.2
  - @pnpm/hooks.types@1101.0.3
  - @pnpm/resolving.resolver-base@1101.3.0
  - @pnpm/store.cafs@1100.3.3
  - @pnpm/store.controller-types@1101.3.0
