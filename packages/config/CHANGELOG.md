# @pnpm/config

## 11.9.0

### Minor Changes

- 8698a7060: New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.

## 11.8.0

### Minor Changes

- fcc1c7100: Add prettier plugins to the default public-hoist-pattern list

## 11.7.2

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/global-bin-dir@1.2.5

## 11.7.1

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1

## 11.7.0

### Minor Changes

- 50b360ec1: A new option added for specifying the shell to use, when running scripts: scriptShell.

## 11.6.1

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0

## 11.6.0

### Minor Changes

- f591fdeeb: New option added: `node-linker`. When `node-linker` is set to `pnp`, pnpm will create a `.pnp.js` file.

## 11.5.0

### Minor Changes

- 74914c178: New experimental option added for installing node_modules w/o symlinks.

## 11.4.0

### Minor Changes

- 23cf3c88b: New option added: `shellEmulator`.

### Patch Changes

- Updated dependencies [846887de3]
  - @pnpm/global-bin-dir@1.2.4

## 11.3.0

### Minor Changes

- 092f8dd83: New setting added: workspace-root.

### Patch Changes

- 767212f4e: Packages like @babel/types should be publicly hoisted by default.

## 11.2.7

### Patch Changes

- 9f1a29ff9: During global install, changes should always be saved to the global package.json, even when save is set to false.
- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/global-bin-dir@1.2.3

## 11.2.6

### Patch Changes

- ac0d3e122: Publicly hoist any dependency that is related to ESLint.

## 11.2.5

### Patch Changes

- 972864e0d: When public-hoist-pattern is set to an empty string or a list with a single empty string, then it is considered to be undefined.
- Updated dependencies [4d4d22b63]
  - @pnpm/global-bin-dir@1.2.2

## 11.2.4

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/global-bin-dir@1.2.1

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
