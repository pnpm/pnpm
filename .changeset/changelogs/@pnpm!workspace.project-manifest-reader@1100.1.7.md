## 1100.1.7

### Patch Changes

- `pnpm pkg` now reads and writes the project manifest as written on disk. `pnpm pkg get` no longer reports synthetic `runtime:` dependency entries from `devEngines.runtime` or `engines.runtime`. `pnpm pkg set` and `pnpm pkg delete` now preserve `devEngines.runtime`, `engines.runtime`, and empty dependency objects set with `--json`.

- Updated dependencies:
  - @pnpm/pkg-manifest.utils@1100.4.13
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.project-manifest-writer@1100.0.23
