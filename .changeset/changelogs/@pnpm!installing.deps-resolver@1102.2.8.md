## 1102.2.8

### Patch Changes

- `pnpm install` no longer fails with `ERR_PNPM_NO_MATCHING_VERSION` after a change to `overrides` when the lockfile resolves an optional peer dependency to an npm alias of another package [#16654](https://github.com/pnpm/pnpm/issues/16654).

- A filtered install, such as `pnpm install --filter <pkg>`, now keeps the `pnpm-lock.yaml` entries of workspace projects that are missing from disk, as in a sparse checkout. Their importers, packages, and catalog entries stay even when the filter matches every project on disk [#16931](https://github.com/pnpm/pnpm/issues/16931).

- pnpm no longer drops a dependency that a package also declares as an optional peer dependency, for example `ajv` of `ajv-formats`. Since 11.28.0, in some dependency graphs the package got the dependency neither as a dependency nor as a peer, and failed to import it [#16901](https://github.com/pnpm/pnpm/issues/16901).

- `pnpm install` and `pnpm dedupe` now link an optional peer to the workspace package that the workspace root depends on when the picked version matches it. Previously they installed the registry package with the same name and version [pnpm/pnpm#16706](https://github.com/pnpm/pnpm/issues/16706).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.8
  - @pnpm/resolving.npm-resolver@1104.2.6
