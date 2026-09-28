## 1100.3.5

### Patch Changes

- Relative local tarball paths in `pnpm.overrides` without an explicit `file:` prefix are now rebased correctly for workspace packages [#11131](https://github.com/pnpm/pnpm/issues/11131).

- pnpm's built-in package compatibility database no longer applies to a project's own manifest. A project named like a published package, such as `vue-loader`, no longer gains dependencies on `pnpm install` or `pnpm update`. User-configured `packageExtensions` still apply to project manifests [#11700](https://github.com/pnpm/pnpm/issues/11700).

- Updated dependencies:
  - @pnpm/config.parse-overrides@1100.1.7
  - @pnpm/error@1100.2.1
  - @pnpm/resolving.local-resolver@1101.2.4
  - @pnpm/resolving.parse-wanted-dependency@1100.0.3
