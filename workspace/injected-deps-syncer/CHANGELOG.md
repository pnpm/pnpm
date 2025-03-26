# @pnpm/workspace.injected-deps-syncer

## 1000.0.4

### Patch Changes

- Updated dependencies [64f6b4f]
  - @pnpm/modules-yaml@1000.3.0
  - @pnpm/directory-fetcher@1000.1.3

## 1000.0.3

### Patch Changes

- Updated dependencies [d612dcf]
- Updated dependencies [d612dcf]
  - @pnpm/modules-yaml@1000.2.0
  - @pnpm/directory-fetcher@1000.1.2

## 1000.0.2

### Patch Changes

- 9904675: `@pnpm/logger` should be a peer dependency.

## 1000.0.1

### Patch Changes

- @pnpm/directory-fetcher@1000.1.1
- @pnpm/modules-yaml@1000.1.4

## 1000.0.0

### Major Changes

- e32b1a2: Added support for automatically syncing files of injected workspace packages after `pnpm run` [#9081](https://github.com/pnpm/pnpm/issues/9081). Use the `sync-injected-deps-after-scripts` setting to specify which scripts build the workspace package. This tells pnpm when syncing is needed. The setting should be defined in a `.npmrc` file at the root of the workspace. Example:

  ```ini
  sync-injected-deps-after-scripts[]=compile
  ```

- e32b1a2: Initial Release.

### Patch Changes

- Updated dependencies [e32b1a2]
  - @pnpm/directory-fetcher@1000.1.0
  - @pnpm/modules-yaml@1000.1.3
