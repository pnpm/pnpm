# @pnpm/git-resolver

## 9.0.4

### Patch Changes

- Updated dependencies [dd00eeb]
  - @pnpm/resolver-base@13.0.0
  - @pnpm/fetch@8.0.3

## 9.0.3

### Patch Changes

- @pnpm/fetch@8.0.2
- @pnpm/resolver-base@12.0.2

## 9.0.2

### Patch Changes

- @pnpm/fetch@8.0.1
- @pnpm/resolver-base@12.0.1

## 9.0.1

### Patch Changes

- c969f37: Lockfiles that have git-hosted dependencies specified should be correctly converted to the new lockfile format [#7990](https://github.com/pnpm/pnpm/issues/7990).

## 9.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- b13d2dc: It is now possible to install only a subdirectory from a Git repository.

  For example, `pnpm add github:user/repo#path:packages/foo` will add a dependency from the `packages/foo` subdirectory.

  This new parameter may be combined with other supported parameters separated by `&`. For instance, the next command will install the same package from the `dev` branch: `pnpm add github:user/repo#dev&path:packages/bar`.

  Related issue: [#4765](https://github.com/pnpm/pnpm/issues/4765).
  Related PR: [#7487](https://github.com/pnpm/pnpm/pull/7487).

### Patch Changes

- 985381c: Install gitlab-hosted packages correctly, when they are specified by commit or branch [#7603](https://github.com/pnpm/pnpm/issues/7603).
- Updated dependencies [7733f3a]
- Updated dependencies [43cdd87]
- Updated dependencies [b13d2dc]
  - @pnpm/fetch@8.0.0
  - @pnpm/resolver-base@12.0.0

## 8.0.12

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/resolver-base@11.1.0

## 8.0.11

### Patch Changes

- @pnpm/resolver-base@11.0.2
- @pnpm/fetch@7.0.7

## 8.0.10

### Patch Changes

- @pnpm/resolver-base@11.0.1
- @pnpm/fetch@7.0.6

## 8.0.9

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/resolver-base@11.0.0

## 8.0.8

### Patch Changes

- @pnpm/resolver-base@10.0.4
- @pnpm/fetch@7.0.5

## 8.0.7

### Patch Changes

- @pnpm/resolver-base@10.0.3
- @pnpm/fetch@7.0.4

## 8.0.6

### Patch Changes

- 22bbe9255: Pass the right scheme to `git ls-remote` in order to prevent a fallback to `git+ssh` that would result in a 'host key verification failed' issue [#6806](https://github.com/pnpm/pnpm/issues/6806)

## 8.0.5

### Patch Changes

- de9b6c20d: Temporarily revert the fix to [#6805](https://github.com/pnpm/pnpm/issues/6805) to fix the regression it caused [#6827](https://github.com/pnpm/pnpm/issues/6827).

## 8.0.4

### Patch Changes

- 6fe0b60e6: Fixed a bug in which pnpm passed the wrong scheme to `git ls-remote`, causing a fallback to `git+ssh` and resulting in a 'host key verification failed' issue [#6805](https://github.com/pnpm/pnpm/issues/6805)
  - @pnpm/resolver-base@10.0.2
  - @pnpm/fetch@7.0.3

## 8.0.3

### Patch Changes

- @pnpm/resolver-base@10.0.1
- @pnpm/fetch@7.0.2

## 8.0.2

### Patch Changes

- Updated dependencies [8228c2cb1]
  - @pnpm/fetch@7.0.1

## 8.0.1

### Patch Changes

- c0760128d: bump semver to 7.4.0

## 8.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- 28796377c: Fix git-hosted dependencies referenced via `git+ssh` that use semver selectors [#6239](https://github.com/pnpm/pnpm/pull/6239).
- Updated dependencies [eceaa8b8b]
  - @pnpm/resolver-base@10.0.0
  - @pnpm/fetch@7.0.0

## 7.0.7

### Patch Changes

- Updated dependencies [673e23060]
- Updated dependencies [9fa6c7404]
  - @pnpm/fetch@6.0.6

## 7.0.6

### Patch Changes

- Updated dependencies [029143cff]
- Updated dependencies [029143cff]
  - @pnpm/resolver-base@9.2.0

## 7.0.5

### Patch Changes

- @pnpm/resolver-base@9.1.5
- @pnpm/fetch@6.0.5

## 7.0.4

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/fetch@6.0.4

## 7.0.3

### Patch Changes

- @pnpm/resolver-base@9.1.4
- @pnpm/fetch@6.0.3

## 7.0.2

### Patch Changes

- @pnpm/fetch@6.0.2

## 7.0.1

### Patch Changes

- @pnpm/resolver-base@9.1.3
- @pnpm/fetch@6.0.1

## 7.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/fetch@6.0.0

## 6.1.7

### Patch Changes

- @pnpm/fetch@5.0.10

## 6.1.6

### Patch Changes

- @pnpm/resolver-base@9.1.2
- @pnpm/fetch@5.0.9

## 6.1.5

### Patch Changes

- @pnpm/resolver-base@9.1.1
- @pnpm/fetch@5.0.8

## 6.1.4

### Patch Changes

- Updated dependencies [23984abd1]
  - @pnpm/resolver-base@9.1.0

## 6.1.3

### Patch Changes

- 39c040127: upgrade various dependencies

## 6.1.2

### Patch Changes

- @pnpm/resolver-base@9.0.6
- @pnpm/fetch@5.0.7

## 6.1.1

### Patch Changes

- Updated dependencies [e018a8b14]
  - @pnpm/fetch@5.0.6

## 6.1.0

### Minor Changes

- 449ccef09: Add `refs/` to git resolution prefixes

## 6.0.6

### Patch Changes

- @pnpm/resolver-base@9.0.5
- @pnpm/fetch@5.0.5

## 6.0.5

### Patch Changes

- @pnpm/resolver-base@9.0.4
- @pnpm/fetch@5.0.4

## 6.0.4

### Patch Changes

- Updated dependencies [9d5bf09c0]
  - @pnpm/fetch@5.0.3
  - @pnpm/resolver-base@9.0.3

## 6.0.3

### Patch Changes

- @pnpm/resolver-base@9.0.2
- @pnpm/fetch@5.0.2

## 6.0.2

### Patch Changes

- 0fa446d10: Resolve commits from GitHub via https.

## 6.0.1

### Patch Changes

- @pnpm/resolver-base@9.0.1
- @pnpm/fetch@5.0.1

## 6.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/fetch@5.0.0
  - @pnpm/resolver-base@9.0.0

## 5.1.17

### Patch Changes

- @pnpm/resolver-base@8.1.6
- @pnpm/fetch@4.2.5

## 5.1.16

### Patch Changes

- @pnpm/resolver-base@8.1.5
- @pnpm/fetch@4.2.4

## 5.1.15

### Patch Changes

- @pnpm/resolver-base@8.1.4
- @pnpm/fetch@4.2.3

## 5.1.14

### Patch Changes

- @pnpm/resolver-base@8.1.3
- @pnpm/fetch@4.2.2

## 5.1.13

### Patch Changes

- c94104472: Don't make unnecessary retries when fetching Git-hosted packages [#2731](https://github.com/pnpm/pnpm/pull/2731).
  - @pnpm/fetch@4.2.1
  - @pnpm/resolver-base@8.1.2

## 5.1.12

### Patch Changes

- Updated dependencies [f1c194ded]
  - @pnpm/fetch@4.2.0

## 5.1.11

### Patch Changes

- Updated dependencies [12ee3c144]
  - @pnpm/fetch@4.1.6

## 5.1.10

### Patch Changes

- @pnpm/resolver-base@8.1.1
- @pnpm/fetch@4.1.5

## 5.1.9

### Patch Changes

- 7da65bd7a: Don't break URLs with ports.

## 5.1.8

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/resolver-base@8.1.0
  - @pnpm/fetch@4.1.4

## 5.1.7

### Patch Changes

- Updated dependencies [782ef2490]
  - @pnpm/fetch@4.1.3

## 5.1.6

### Patch Changes

- 930e104da: Git URLs containing a colon should work.
  - @pnpm/fetch@4.1.2

## 5.1.5

### Patch Changes

- 04b7f6086: Use safe-execa instead of execa to prevent binary planting attacks on Windows.

## 5.1.4

### Patch Changes

- Updated dependencies [bab172385]
  - @pnpm/fetch@4.1.1

## 5.1.3

### Patch Changes

- Updated dependencies [eadf0e505]
  - @pnpm/fetch@4.1.0

## 5.1.2

### Patch Changes

- @pnpm/resolver-base@8.0.4
- @pnpm/fetch@4.0.2

## 5.1.1

### Patch Changes

- @pnpm/resolver-base@8.0.3
- @pnpm/fetch@4.0.1

## 5.1.0

### Minor Changes

- 69ffc4099: It should be possible to install a Git-hosted dependency that names the default branch not "master".

## 5.0.2

### Patch Changes

- Updated dependencies [e7d9cd187]
- Updated dependencies [eeff424bd]
  - @pnpm/fetch@4.0.0
  - @pnpm/resolver-base@8.0.2

## 5.0.1

### Patch Changes

- Updated dependencies [05baaa6e7]
  - @pnpm/fetch@3.1.0
  - @pnpm/resolver-base@8.0.1

## 5.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
  - @pnpm/fetch@3.0.0
  - @pnpm/resolver-base@8.0.0

## 4.1.12

### Patch Changes

- @pnpm/fetch@2.1.11

## 4.1.11

### Patch Changes

- @pnpm/resolver-base@7.1.1
- @pnpm/fetch@2.1.10

## 4.1.10

### Patch Changes

- 32c9ef4be: execa updated to v5.

## 4.1.9

### Patch Changes

- @pnpm/fetch@2.1.9

## 4.1.8

### Patch Changes

- Updated dependencies [263f5d813]
  - @pnpm/fetch@2.1.8

## 4.1.7

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/resolver-base@7.1.0

## 4.1.6

### Patch Changes

- @pnpm/resolver-base@7.0.5
- @pnpm/fetch@2.1.7

## 4.1.5

### Patch Changes

- @pnpm/resolver-base@7.0.4
- @pnpm/fetch@2.1.6

## 4.1.4

### Patch Changes

- @pnpm/fetch@2.1.5

## 4.1.3

### Patch Changes

- Updated dependencies [3981f5558]
  - @pnpm/fetch@2.1.4

## 4.1.2

### Patch Changes

- @pnpm/fetch@2.1.3

## 4.1.1

### Patch Changes

- @pnpm/fetch@2.1.2

## 4.1.0

### Minor Changes

- 2ebcfc38a: Installation of private Git-hosted repositories via HTTPS, using an auth token.

### Patch Changes

- 7b98d16c8: Update lru-cache to v6
  - @pnpm/fetch@2.1.1

## 4.0.16

### Patch Changes

- Updated dependencies [71aeb9a38]
  - @pnpm/fetch@2.1.0

## 4.0.15

### Patch Changes

- @pnpm/resolver-base@7.0.3
- @pnpm/fetch@2.0.2

## 4.0.14

### Patch Changes

- @pnpm/resolver-base@7.0.2
- @pnpm/fetch@2.0.1

## 4.0.13

### Patch Changes

- Updated dependencies [2ebb7af33]
  - @pnpm/fetch@2.0.0

## 4.0.12

### Patch Changes

- @pnpm/fetch@1.0.4
- @pnpm/resolver-base@7.0.1

## 4.0.12-alpha.0

### Patch Changes

- @pnpm/resolver-base@7.0.1-alpha.0
