# @pnpm/builder.policy

## 1000.0.0

### Major Changes

- dee39ec: Sync version with pnpm CLI.

### Minor Changes

- dee39ec: You can now allow specific versions of dependencies to run postinstall scripts. `onlyBuiltDependencies` now accepts package names with lists of trusted versions. For example:

  ```yaml
  onlyBuiltDependencies:
    - nx@21.6.4 || 21.6.5
    - esbuild@0.25.1
  ```

  Related PR: [#10104](https://github.com/pnpm/pnpm/pull/10104).

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0
  - @pnpm/config.version-policy@1000.0.0
