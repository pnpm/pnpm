## 1100.0.38

### Patch Changes

- `pnpm deploy` with `deployAllFiles` now rejects symlinks that point outside the package directory. Local package installs with this setting apply the same check.

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.20
  - @pnpm/error@1100.2.2
  - @pnpm/fs.packlist@1100.0.8
  - @pnpm/workspace.project-manifest-reader@1100.1.3
