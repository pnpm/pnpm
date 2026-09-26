## 1100.3.3

### Patch Changes

- The `publish` command now resolves `workspace:` dependencies from workspace manifests when `node_modules` is not installed. Previously, publishing without `node_modules` failed with `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL`. pnpm/pnpm#6567

- `make-dedicated-lockfile` no longer removes fields such as `main` and `types` from the `publishConfig` of the project's `package.json`.

- `pnpm publish` and `pnpm pack` now report a missing `version` or `name` field on a workspace dependency. Previously, pnpm reported that the dependency was not installed [#4164](https://github.com/pnpm/pnpm/issues/4164).

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.17
  - @pnpm/error@1100.2.0
  - @pnpm/resolving.jsr-specifier-parser@1100.0.8
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
