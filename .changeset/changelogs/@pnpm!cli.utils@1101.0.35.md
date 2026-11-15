## 1101.0.35

### Patch Changes

- `pnpm install` now fails on a workspace project's incompatible `engines.pnpm`, or on its incompatible `engines.node` with `engineStrict` enabled, even when the project's `os`, `cpu`, or `libc` excludes the current platform [pnpm/tasks#281](https://github.com/pnpm/tasks/issues/281).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.6
  - @pnpm/pkg-manifest.utils@1100.4.12
  - @pnpm/workspace.project-manifest-reader@1100.1.6
