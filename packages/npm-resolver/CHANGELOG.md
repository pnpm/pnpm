# @pnpm/npm-resolver

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
