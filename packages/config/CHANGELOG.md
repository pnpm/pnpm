# @pnpm/config

## 11.2.3

### Patch Changes

- 13c18e397: Stop searching for local prefix, when directory has a `package.json5` or `package.yaml`.

## 11.2.2

### Patch Changes

- 3f6d35997: Don't read the `.npmrc` files that are outside of the current workspace.

## 11.2.1

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 11.2.0

### Minor Changes

- ad69677a7: A new option added that allows to resolve the global bin directory from directories to which there is no write access.

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/global-bin-dir@1.2.0

## 11.1.0

### Minor Changes

- 65b4d07ca: feat: add config to make install only install package dependencies in a workspace
- ab3b8f51d: Hoist all ESLint plugins to the root of node_modules by default.

## 11.0.1

### Patch Changes

- Updated dependencies [245221baa]
  - @pnpm/global-bin-dir@1.1.1

## 11.0.0

### Major Changes

- 71aeb9a38: Remove proxy from the object returned by @pnpm/config. httpsProxy and httpProxy are returned instead.

### Minor Changes

- 915828b46: A new setting is returned by `@pnpm/config`: `npmGlobalBinDir`.
  `npmGlobalBinDir` is the global executable directory used by npm.

  This new config is used by `@pnpm/global-bin-dir` to find a suitable
  directory for the binstubs installed by pnpm globally.

### Patch Changes

- Updated dependencies [915828b46]
  - @pnpm/global-bin-dir@1.1.0

## 10.0.1

### Patch Changes

- Updated dependencies [2c190d49d]
  - @pnpm/global-bin-dir@1.0.1

## 10.0.0

### Major Changes

- db17f6f7b: Move Project and ProjectsGraph to @pnpm/types.
- 1146b76d2: `globalBin` is removed from the returned object.

  The value of `bin` is set by the `@pnpm/global-bin-dir` package when the `--global` option is used.

### Patch Changes

- Updated dependencies [1146b76d2]
- Updated dependencies [db17f6f7b]
  - @pnpm/global-bin-dir@1.0.0
  - @pnpm/types@6.2.0

## 9.2.0

### Minor Changes

- 71a8c8ce3: Added a new setting: `public-hoist-pattern`. This setting can be overwritten by `--[no-]shamefully-hoist`. The default value of `public-hoist-pattern` is `types/*`.

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0

## 9.1.0

### Minor Changes

- ffddf34a8: Add new global option called `--stream`.
  When used, the output from child processes is streamed to the console immediately, prefixed with the originating package directory. This allows output from different packages to be interleaved.

## 9.0.0

### Major Changes

- e11019b89: Deprecate the resolution strategy setting. The fewer dependencies strategy is used always.
- 802d145fc: Remove `independent-leaves` support.
- 45fdcfde2: Locking is removed.

### Minor Changes

- 242cf8737: The `link-workspace-packages` setting may be set to `deep`. When using `deep`,
  workspace packages are linked into subdependencies, not only to direct dependencies of projects.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [da091c711]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/types@6.0.0
  - @pnpm/error@1.2.1

## 9.0.0-alpha.2

### Major Changes

- 45fdcfde2: Locking is removed.

### Minor Changes

- 242cf8737: The `link-workspace-packages` setting may be set to `deep`. When using `deep`,
  workspace packages are linked into subdependencies, not only to direct dependencies of projects.

### Patch Changes

- Updated dependencies [ca9f50844]
  - @pnpm/constants@4.0.0-alpha.1

## 8.3.1-alpha.1

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0

## 8.3.1-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
