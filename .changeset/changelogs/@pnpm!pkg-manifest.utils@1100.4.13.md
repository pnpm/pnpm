## 1100.4.13

### Patch Changes

- `pnpm pkg` now reads and writes the project manifest as written on disk. `pnpm pkg get` no longer reports synthetic `runtime:` dependency entries from `devEngines.runtime` or `engines.runtime`. `pnpm pkg set` and `pnpm pkg delete` now preserve `devEngines.runtime`, `engines.runtime`, and empty dependency objects set with `--json`.

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.5
  - @pnpm/types@1102.1.4
