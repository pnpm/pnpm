# @pnpm/npm-resolver

## 13.1.8

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/core-loggers@7.0.7
  - @pnpm/resolver-base@9.1.1

## 13.1.7

### Patch Changes

- a3ccd27a3: `@types/ramda` should be a dev dependency.

## 13.1.6

### Patch Changes

- d7fc07cc7: Include `hasInstallScript` in the abbreviated metadata.

## 13.1.5

### Patch Changes

- 7fac3b446: Pick a version even if it was published after the given date (if there is no better match).

## 13.1.4

### Patch Changes

- 53506c7ae: Don't modify the manifest of the injected workspace project, when it has the same dependency in prod and peer dependencies.

## 13.1.3

### Patch Changes

- dbac0ca01: Update ssri to v9.

## 13.1.2

### Patch Changes

- Updated dependencies [23984abd1]
  - @pnpm/resolver-base@9.1.0

## 13.1.1

### Patch Changes

- 238a165a5: dependencies maintenance

## 13.1.0

### Minor Changes

- c90798461: When `publishConfig.directory` is set, only symlink it to other workspace projects if `publishConfig.linkDirectory` is set to `true`. Otherwise, only use it for publishing [#5115](https://github.com/pnpm/pnpm/issues/5115).

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/core-loggers@7.0.6
  - @pnpm/resolver-base@9.0.6

## 13.0.7

### Patch Changes

- eb2426cf8: When a project in a workspace has a `publishConfig.directory` set, dependent projects should install the project from that directory [#3901](https://github.com/pnpm/pnpm/issues/3901)

## 13.0.6

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/core-loggers@7.0.5
  - @pnpm/resolver-base@9.0.5

## 13.0.5

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/core-loggers@7.0.4
  - @pnpm/resolver-base@9.0.4

## 13.0.4

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0
  - @pnpm/core-loggers@7.0.3
  - @pnpm/resolver-base@9.0.3

## 13.0.3

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/core-loggers@7.0.2
  - @pnpm/resolver-base@9.0.2

## 13.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/core-loggers@7.0.1
  - @pnpm/resolver-base@9.0.1

## 13.0.1

### Patch Changes

- @pnpm/error@3.0.1

## 13.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/error@3.0.0
  - @pnpm/fetching-types@3.0.0
  - @pnpm/graceful-fs@2.0.0
  - @pnpm/resolve-workspace-range@3.0.0
  - @pnpm/resolver-base@9.0.0

## 12.1.8

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0

## 12.1.7

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/core-loggers@6.1.4
  - @pnpm/resolver-base@8.1.6

## 12.1.6

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/core-loggers@6.1.3
  - @pnpm/resolver-base@8.1.5

## 12.1.5

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/core-loggers@6.1.2
  - @pnpm/resolver-base@8.1.4

## 12.1.4

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/core-loggers@6.1.1
  - @pnpm/resolver-base@8.1.3

## 12.1.3

### Patch Changes

- 81ed15666: Always add a trailing slash to the registry URL [#4052](https://github.com/pnpm/pnpm/issues/4052).
- Updated dependencies [ba9b2eba1]
- Updated dependencies [ba9b2eba1]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/types@7.7.0
  - @pnpm/resolver-base@8.1.2

## 12.1.2

### Patch Changes

- 9f61bd81b: Downgrading `p-memoize` to v4.0.1. pnpm v6.22.0 started to print the next warning [#3989](https://github.com/pnpm/pnpm/issues/3989):

  ```
  (node:132923) TimeoutOverflowWarning: Infinity does not fit into a 32-bit signed integer.
  ```

## 12.1.1

### Patch Changes

- 108bd4a39: Injected directory resolutions should contain the relative path to the directory.
- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/core-loggers@6.0.6
  - @pnpm/resolver-base@8.1.1

## 12.1.0

### Minor Changes

- 4ab87844a: Support the resolution of injected local dependencies.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/resolver-base@8.1.0
  - @pnpm/core-loggers@6.0.5

## 12.0.5

### Patch Changes

- 82caa0b56: It should be possible to alias scoped packages using the `workspace:` protocol. See https://github.com/pnpm/pnpm/issues/3883

## 12.0.4

### Patch Changes

- Updated dependencies [bab172385]
  - @pnpm/fetching-types@2.2.1

## 12.0.3

### Patch Changes

- eadf0e505: The metadata file should be requested in compressed state.
- Updated dependencies [eadf0e505]
  - @pnpm/fetching-types@2.2.0

## 12.0.2

### Patch Changes

- a4fed2798: Do not fail if a package has no shasum in the metadata.

  Fail if a package has broken shasum in the metadata.

## 12.0.1

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/core-loggers@6.0.4
  - @pnpm/resolver-base@8.0.4

## 12.0.0

### Major Changes

- 691f64713: New required option added: cacheDir.

## 11.1.4

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/core-loggers@6.0.3
  - @pnpm/resolver-base@8.0.3

## 11.1.3

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/core-loggers@6.0.2
  - @pnpm/resolver-base@8.0.2

## 11.1.2

### Patch Changes

- ae36ac7d3: Fix: unhandled rejection in npm resolver when fetch fails
- bf322c702: Avoid conflicts in metadata, when a package name has upper case letters.

## 11.1.1

### Patch Changes

- Updated dependencies [a2aeeef88]
  - @pnpm/graceful-fs@1.0.0

## 11.1.0

### Minor Changes

- 85fb21a83: Add support for workspace:^ and workspace:~ aliases
- 05baaa6e7: Add new option: timeout.

### Patch Changes

- Updated dependencies [85fb21a83]
- Updated dependencies [05baaa6e7]
- Updated dependencies [97c64bae4]
  - @pnpm/resolve-workspace-range@2.1.0
  - @pnpm/fetching-types@2.1.0
  - @pnpm/types@7.1.0
  - @pnpm/core-loggers@6.0.1
  - @pnpm/resolver-base@8.0.1

## 11.0.1

### Patch Changes

- 6f198457d: Update rename-overwrite.

## 11.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 83645c8ed: Update ssri.
- Updated dependencies [97b986fbc]
- Updated dependencies [90487a3a8]
  - @pnpm/core-loggers@6.0.0
  - @pnpm/error@2.0.0
  - @pnpm/fetching-types@2.0.0
  - @pnpm/resolve-workspace-range@2.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/types@7.0.0

## 10.2.2

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/core-loggers@5.0.3
  - @pnpm/resolver-base@7.1.1

## 10.2.1

### Patch Changes

- f47551a3c: Throw a meaningful error on malformed registry metadata.

## 10.2.0

### Minor Changes

- 8698a7060: New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/resolver-base@7.1.0

## 10.1.0

### Minor Changes

- 284e95c5e: Skip workspace protocol specs that use relative path.
- 084614f55: Support aliases to workspace packages. For instance, `"foo": "workspace:bar@*"` will link bar from the repository but aliased to foo. Before publish, these specs are converted to regular aliased versions.

## 10.0.7

### Patch Changes

- 5ff6c28fa: Retry metadata download if the received JSON is broken.
- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0

## 10.0.6

### Patch Changes

- 39142e2ad: Update encode-registry to v3.

## 10.0.5

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/resolver-base@7.0.5

## 10.0.4

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/resolver-base@7.0.4

## 10.0.3

### Patch Changes

- d7b727795: Update p-memoize to v4.0.1.

## 10.0.2

### Patch Changes

- 3633f5e46: When no matching version is found, report the actually specified version spec in the error message (not the normalized one).

## 10.0.1

### Patch Changes

- 75a36deba: Report information about any used auth token, if an error happens during fetch.
- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1

## 10.0.0

### Major Changes

- a1cdae3dc: Does not accept a `metaCache` option anymore. Caching happens internally, using `lru-cache`.

## 9.1.0

### Minor Changes

- 6d480dd7a: Report whether/what authorization header was used to make the request, when the request fails with an authorization issue.

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0

## 9.0.2

### Patch Changes

- 622c0b6f9: Always use the package name that is given at the root of the metadata object. Override any names that are specified in the version manifests. This fixes an issue with GitHub registry.
- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 9.0.1

### Patch Changes

- 379cdcaf8: When resolution from workspace fails, print the path to the project that has the unsatisfied dependency.

## 9.0.0

### Major Changes

- 71aeb9a38: Breaking changes to the API. fetchFromRegistry and getCredentials are passed in through arguments.

### Patch Changes

- Updated dependencies [71aeb9a38]
  - @pnpm/fetching-types@1.0.0

## 8.1.2

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/resolver-base@7.0.3
  - fetch-from-npm-registry@4.1.2

## 8.1.1

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/resolver-base@7.0.2
  - fetch-from-npm-registry@4.1.1

## 8.1.0

### Minor Changes

- 4cf7ef367: Reducing filesystem operations required to write the metadata file to the cache.

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [2ebb7af33]
  - fetch-from-npm-registry@4.1.0

## 8.0.1

### Patch Changes

- Updated dependencies [872f81ca1]
  - fetch-from-npm-registry@4.0.3

## 8.0.0

### Major Changes

- 5bc033c43: Reduce the number of directories in the store by keeping all the metadata json files in the same directory.

### Patch Changes

- f453a5f46: Update version-selector-type to v3.
- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0
  - @pnpm/error@1.2.1
  - fetch-from-npm-registry@4.0.3
  - @pnpm/resolve-workspace-range@1.0.2
  - @pnpm/resolver-base@7.0.1

## 8.0.0-alpha.2

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0

## 8.0.0-alpha.1

### Major Changes

- 5bc033c43: Reduce the number of directories in the store by keeping all the metadata json files in the same directory.

## 7.3.12-alpha.0

### Patch Changes

- f453a5f46: Update version-selector-type to v3.
