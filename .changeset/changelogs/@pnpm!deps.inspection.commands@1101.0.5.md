## 1101.0.5

### Patch Changes

- Install warnings no longer carry the text of a package's deprecation notice. The warning names the deprecated package and version, and the `pnpm:deprecation` event no longer carries the notice either. `pnpm view` still shows it on request.

  A deprecation warning now names the newest version of the package that is not deprecated, and says when reaching it means widening the range you declared:

  ```
  WARN  deprecated foo@1.0.0. 2.3.1 is not deprecated, outside the range you declared.
  ```

  pnpm works this out from the metadata it already fetched, so it costs no extra request. An install that reuses the lockfile without fetching metadata names no version.

  pnpm strips control characters from the package name and version in a deprecation warning, and from the notice `pnpm outdated --long` prints.

  The text sanitizer now also strips the Unicode line and paragraph separators U+2028 and U+2029.

- `pnpm -r list --json` now prints one JSON array. It printed a separate array for each project when `sharedWorkspaceLockfile` was `false`, so the output could not be parsed.

  `pnpm -r list` now reads each project's own modules directory when the projects keep their own lockfiles, so `--long` and `--parseable` report the packages that project installed [#15011](https://github.com/pnpm/pnpm/issues/15011).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.28
  - @pnpm/config.reader@1102.2.1
  - @pnpm/deps.github-actions@1100.1.10
  - @pnpm/deps.inspection.list@1101.0.5
  - @pnpm/deps.inspection.outdated@1100.1.31
  - @pnpm/deps.inspection.peers-checker@1100.0.34
  - @pnpm/global.commands@1102.0.2
  - @pnpm/global.packages@1101.1.3
  - @pnpm/installing.modules-yaml@1101.0.3
  - @pnpm/lockfile.fs@1100.2.8
  - @pnpm/network.fetch@1100.1.17
  - @pnpm/resolving.default-resolver@1101.0.5
  - @pnpm/resolving.npm-resolver@1104.2.0
  - @pnpm/text.sanitize@1100.0.1
  - @pnpm/workspace.project-manifest-reader@1100.0.29
