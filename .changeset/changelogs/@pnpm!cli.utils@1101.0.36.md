## 1101.0.36

### Patch Changes

- `pnpm pkg` now reads and writes the project manifest as written on disk. `pnpm pkg get` no longer reports synthetic `runtime:` dependency entries from `devEngines.runtime` or `engines.runtime`. `pnpm pkg set` and `pnpm pkg delete` now preserve `devEngines.runtime`, `engines.runtime`, and empty dependency objects set with `--json`.

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.5
  - @pnpm/config.package-is-installable@1100.2.7
  - @pnpm/pkg-manifest.utils@1100.4.13
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.project-manifest-reader@1100.1.7
