# @pnpm/directory-fetcher

## 8.0.4

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/resolver-base@13.0.0
  - @pnpm/types@11.0.0
  - @pnpm/fetcher-base@16.0.3
  - @pnpm/exec.pkg-requires-build@1.0.3
  - @pnpm/read-project-manifest@6.0.4

## 8.0.3

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/exec.pkg-requires-build@1.0.2
  - @pnpm/fetcher-base@16.0.2
  - @pnpm/read-project-manifest@6.0.3
  - @pnpm/resolver-base@12.0.2

## 8.0.2

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/exec.pkg-requires-build@1.0.1
  - @pnpm/fetcher-base@16.0.1
  - @pnpm/read-project-manifest@6.0.2
  - @pnpm/resolver-base@12.0.1

## 8.0.1

### Patch Changes

- @pnpm/read-project-manifest@6.0.1

## 8.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [43cdd87]
- Updated dependencies [0e6b757]
- Updated dependencies [b13d2dc]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/read-project-manifest@6.0.0
  - @pnpm/resolver-base@12.0.0
  - @pnpm/fetcher-base@16.0.0
  - @pnpm/fs.packlist@2.0.0
  - @pnpm/exec.pkg-requires-build@1.0.0

## 7.0.11

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/resolver-base@11.1.0
  - @pnpm/fetcher-base@15.0.7

## 7.0.10

### Patch Changes

- Updated dependencies [9fb45d0fc]
  - @pnpm/fs.packlist@1.0.3

## 7.0.9

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2
  - @pnpm/fetcher-base@15.0.6
  - @pnpm/read-project-manifest@5.0.10
  - @pnpm/resolver-base@11.0.2

## 7.0.8

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/fetcher-base@15.0.5
  - @pnpm/read-project-manifest@5.0.9
  - @pnpm/resolver-base@11.0.1

## 7.0.7

### Patch Changes

- Updated dependencies [74432d605]
  - @pnpm/fs.packlist@1.0.2

## 7.0.6

### Patch Changes

- Updated dependencies [c7f1359b6]
  - @pnpm/fs.packlist@1.0.1

## 7.0.5

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/resolver-base@11.0.0
  - @pnpm/fetcher-base@15.0.4

## 7.0.4

### Patch Changes

- 500363647: `pnpm publish` should not pack the same file twice sometimes [#6997](https://github.com/pnpm/pnpm/issues/6997).

  The fix was to update `npm-packlist` to the latest version.

- Updated dependencies [500363647]
  - @pnpm/fs.packlist@1.0.0

## 7.0.3

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/fetcher-base@15.0.3
  - @pnpm/read-project-manifest@5.0.8
  - @pnpm/resolver-base@10.0.4

## 7.0.2

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/fetcher-base@15.0.2
  - @pnpm/read-project-manifest@5.0.7
  - @pnpm/resolver-base@10.0.3

## 7.0.1

### Patch Changes

- @pnpm/fetcher-base@15.0.1
- @pnpm/read-project-manifest@5.0.6

## 7.0.0

### Major Changes

- 4a1a9431d: Breaking change to the `directory-fetcher` API.

### Patch Changes

- d92070876: Reverting a change shipped in v8.7 that caused issues with the `pnpm deploy` command and "injected dependencies" [#6943](https://github.com/pnpm/pnpm/pull/6943).
- Updated dependencies [4a1a9431d]
  - @pnpm/fetcher-base@15.0.1

## 6.1.0

### Minor Changes

- d57e4de6d: Apply `publishConfig` for workspace packages on directory fetch. Enables a publishable ("exportable") `package.json` on deployment [#6693](https://github.com/pnpm/pnpm/issues/6693).

### Patch Changes

- Updated dependencies [70b2830ac]
- Updated dependencies [e9aa6f682]
- Updated dependencies [083bbf590]
  - @pnpm/fetcher-base@15.0.0
  - @pnpm/exportable-manifest@5.0.6
  - @pnpm/read-project-manifest@5.0.5

## 6.0.4

### Patch Changes

- @pnpm/fetcher-base@14.0.2
- @pnpm/read-project-manifest@5.0.4
- @pnpm/resolver-base@10.0.2

## 6.0.3

### Patch Changes

- Updated dependencies [b4892acc5]
  - @pnpm/read-project-manifest@5.0.3

## 6.0.2

### Patch Changes

- @pnpm/read-project-manifest@5.0.2

## 6.0.1

### Patch Changes

- @pnpm/fetcher-base@14.0.1
- @pnpm/read-project-manifest@5.0.1
- @pnpm/resolver-base@10.0.1

## 6.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/read-project-manifest@5.0.0
  - @pnpm/resolver-base@10.0.0
  - @pnpm/fetcher-base@14.0.0

## 5.1.6

### Patch Changes

- @pnpm/read-project-manifest@4.1.4

## 5.1.5

### Patch Changes

- Updated dependencies [029143cff]
- Updated dependencies [029143cff]
  - @pnpm/resolver-base@9.2.0
  - @pnpm/fetcher-base@13.1.6

## 5.1.4

### Patch Changes

- @pnpm/read-project-manifest@4.1.3

## 5.1.3

### Patch Changes

- @pnpm/fetcher-base@13.1.5
- @pnpm/read-project-manifest@4.1.2
- @pnpm/resolver-base@9.1.5

## 5.1.2

### Patch Changes

- @pnpm/read-project-manifest@4.1.1

## 5.1.1

### Patch Changes

- Updated dependencies [fec9e3149]
- Updated dependencies [0d12d38fd]
  - @pnpm/read-project-manifest@4.1.0

## 5.1.0

### Minor Changes

- eacff33e4: New option added to resolve symlinks to their real locations, when injecting directories.

## 5.0.0

### Major Changes

- 6710d9dd9: @pnpm/logger added to peer dependencies.

### Patch Changes

- 6710d9dd9: Installation shouldn't fail when the injected dependency has broken symlinks. The broken symlinks should be just skipped [#5598](https://github.com/pnpm/pnpm/issues/5598).
  - @pnpm/fetcher-base@13.1.4
  - @pnpm/read-project-manifest@4.0.2
  - @pnpm/resolver-base@9.1.4

## 4.0.1

### Patch Changes

- @pnpm/fetcher-base@13.1.3
- @pnpm/read-project-manifest@4.0.1
- @pnpm/resolver-base@9.1.3

## 4.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [f884689e0]
  - @pnpm/read-project-manifest@4.0.0

## 3.1.5

### Patch Changes

- @pnpm/read-project-manifest@3.0.13

## 3.1.4

### Patch Changes

- @pnpm/read-project-manifest@3.0.12

## 3.1.3

### Patch Changes

- @pnpm/fetcher-base@13.1.2
- @pnpm/read-project-manifest@3.0.11
- @pnpm/resolver-base@9.1.2

## 3.1.2

### Patch Changes

- @pnpm/fetcher-base@13.1.1
- @pnpm/read-project-manifest@3.0.10
- @pnpm/resolver-base@9.1.1

## 3.1.1

### Patch Changes

- 07bc24ad1: Update npm-packlist.

## 3.1.0

### Minor Changes

- 23984abd1: Add hook for adding custom fetchers.

### Patch Changes

- Updated dependencies [32915f0e4]
- Updated dependencies [23984abd1]
  - @pnpm/fetcher-base@13.1.0
  - @pnpm/resolver-base@9.1.0

## 3.0.10

### Patch Changes

- 39c040127: upgrade various dependencies
- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [39c040127]
  - @pnpm/read-project-manifest@3.0.9

## 3.0.9

### Patch Changes

- @pnpm/fetcher-base@13.0.2
- @pnpm/read-project-manifest@3.0.8
- @pnpm/resolver-base@9.0.6

## 3.0.8

### Patch Changes

- Updated dependencies [01c5834bf]
  - @pnpm/read-project-manifest@3.0.7

## 3.0.7

### Patch Changes

- 5f643f23b: Update ramda to v0.28.

## 3.0.6

### Patch Changes

- @pnpm/fetcher-base@13.0.1
- @pnpm/read-project-manifest@3.0.6
- @pnpm/resolver-base@9.0.5

## 3.0.5

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/fetcher-base@13.0.0
  - @pnpm/read-project-manifest@3.0.5
  - @pnpm/resolver-base@9.0.4

## 3.0.4

### Patch Changes

- Updated dependencies [0abfe1718]
  - @pnpm/fetcher-base@12.1.0
  - @pnpm/read-project-manifest@3.0.4
  - @pnpm/resolver-base@9.0.3

## 3.0.3

### Patch Changes

- @pnpm/fetcher-base@12.0.3
- @pnpm/read-project-manifest@3.0.3
- @pnpm/resolver-base@9.0.2

## 3.0.2

### Patch Changes

- Updated dependencies [6756c2b02]
  - @pnpm/fetcher-base@12.0.2

## 3.0.1

### Patch Changes

- @pnpm/fetcher-base@12.0.1
- @pnpm/read-project-manifest@3.0.2
- @pnpm/resolver-base@9.0.1

## 3.0.0

### Major Changes

- 41cae6450: Fetch all files from the directory by default.

### Patch Changes

- @pnpm/read-project-manifest@3.0.1

## 2.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/fetcher-base@12.0.0
  - @pnpm/read-project-manifest@3.0.0
  - @pnpm/resolver-base@9.0.0

## 1.0.7

### Patch Changes

- @pnpm/read-project-manifest@2.0.13

## 1.0.6

### Patch Changes

- @pnpm/fetcher-base@11.1.6
- @pnpm/read-project-manifest@2.0.12
- @pnpm/resolver-base@8.1.6

## 1.0.5

### Patch Changes

- aa1f9dc19: Don't fail if the linked package has no `package.json` file.
- 4f78a2a5f: Update npm-packlist to v3.
  - @pnpm/fetcher-base@11.1.5
  - @pnpm/read-project-manifest@2.0.11
  - @pnpm/resolver-base@8.1.5

## 1.0.4

### Patch Changes

- @pnpm/fetcher-base@11.1.4
- @pnpm/resolver-base@8.1.4

## 1.0.3

### Patch Changes

- @pnpm/fetcher-base@11.1.3
- @pnpm/resolver-base@8.1.3

## 1.0.2

### Patch Changes

- @pnpm/fetcher-base@11.1.2
- @pnpm/resolver-base@8.1.2

## 1.0.1

### Patch Changes

- 108bd4a39: Installing a workspace project with an injected dependency from a non-root directory should not fail [#3970](https://github.com/pnpm/pnpm/issues/3970).
  - @pnpm/fetcher-base@11.1.1
  - @pnpm/resolver-base@8.1.1

## 1.0.0

### Major Changes

- 4ab87844a: Initial release.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/fetcher-base@11.1.0
  - @pnpm/resolver-base@8.1.0
