## 1101.0.5

### Patch Changes

- `pnpm -r list --json` now prints one JSON array. It printed a separate array for each project when `sharedWorkspaceLockfile` was `false`, so the output could not be parsed.

  `pnpm -r list` now reads each project's own modules directory when the projects keep their own lockfiles, so `--long` and `--parseable` report the packages that project installed [#15011](https://github.com/pnpm/pnpm/issues/15011).

- Updated dependencies:
  - @pnpm/deps.inspection.tree-builder@1101.0.5
  - @pnpm/lockfile.fs@1100.2.8
  - @pnpm/workspace.project-manifest-reader@1100.0.29
