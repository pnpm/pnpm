# @pnpm/get-context

## 12.0.0

### Major Changes

- dd00eeb: Renamed dir to rootDir in the Project object.

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/resolver-base@13.0.0
  - @pnpm/types@11.0.0
  - @pnpm/lockfile-file@9.1.2
  - @pnpm/core-loggers@10.0.3
  - @pnpm/modules-yaml@13.1.3
  - @pnpm/read-projects-context@9.1.6

## 11.2.1

### Patch Changes

- 13e55b2: If install is performed on a subset of workspace projects, always create an up-to-date lockfile first. So, a partial install can be performed only on a fully resolved (non-partial) lockfile [#8165](https://github.com/pnpm/pnpm/issues/8165).
- Updated dependencies [13e55b2]
  - @pnpm/read-projects-context@9.1.5
  - @pnpm/types@10.1.1
  - @pnpm/lockfile-file@9.1.1
  - @pnpm/core-loggers@10.0.2
  - @pnpm/modules-yaml@13.1.2

## 11.2.0

### Minor Changes

- 47341e5: **Semi-breaking.** Dependency key names in the lockfile are shortened if they are longer than 1000 characters. We don't expect this change to affect many users. Affected users most probably can't run install successfully at the moment. This change is required to fix some edge cases in which installation fails with an out-of-memory error or "Invalid string length (RangeError: Invalid string length)" error. The max allowed length of the dependency key can be controlled with the `peers-suffix-max-length` setting [#8177](https://github.com/pnpm/pnpm/pull/8177).

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/lockfile-file@9.1.0
  - @pnpm/read-projects-context@9.1.4

## 11.1.3

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-file@9.0.6
  - @pnpm/core-loggers@10.0.1
  - @pnpm/modules-yaml@13.1.1
  - @pnpm/read-projects-context@9.1.3

## 11.1.2

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/lockfile-file@9.0.5
  - @pnpm/read-projects-context@9.1.2

## 11.1.1

### Patch Changes

- @pnpm/lockfile-file@9.0.4
- @pnpm/read-projects-context@9.1.1

## 11.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/read-projects-context@9.1.0
  - @pnpm/modules-yaml@13.1.0
  - @pnpm/lockfile-file@9.0.3

## 11.0.2

### Patch Changes

- Updated dependencies [c969f37]
  - @pnpm/lockfile-file@9.0.2
  - @pnpm/read-projects-context@9.0.2

## 11.0.1

### Patch Changes

- Updated dependencies [2cbf7b7]
- Updated dependencies [6b6ca69]
  - @pnpm/lockfile-file@9.0.1
  - @pnpm/read-projects-context@9.0.1

## 11.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- 19c4b4f: When purging multiple node_modules folders, pnpm will no longer print multiple prompts simultaneously.
- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [c692f80]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [f67ad31]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/error@6.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/read-projects-context@9.0.0
  - @pnpm/modules-yaml@13.0.0
  - @pnpm/lockfile-file@9.0.0
  - @pnpm/core-loggers@10.0.0

## 10.0.11

### Patch Changes

- 60bcc797f: Registry configuration from previous installation should not override current settings [#7507](https://github.com/pnpm/pnpm/issues/7507).

## 10.0.10

### Patch Changes

- Updated dependencies [d349bc3a2]
  - @pnpm/modules-yaml@12.1.7
  - @pnpm/read-projects-context@8.0.11

## 10.0.9

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2
  - @pnpm/lockfile-file@8.1.6
  - @pnpm/core-loggers@9.0.6
  - @pnpm/modules-yaml@12.1.6
  - @pnpm/read-projects-context@8.0.10

## 10.0.8

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/lockfile-file@8.1.5
  - @pnpm/core-loggers@9.0.5
  - @pnpm/modules-yaml@12.1.5
  - @pnpm/read-projects-context@8.0.9

## 10.0.7

### Patch Changes

- b1fd38cca: The modules directory should not be removed if the registry configuration has changed.

## 10.0.6

### Patch Changes

- 2143a9388: Improve the error message when `node_modules` should be recreated.

## 10.0.5

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/lockfile-file@8.1.4
  - @pnpm/core-loggers@9.0.4
  - @pnpm/modules-yaml@12.1.4
  - @pnpm/read-projects-context@8.0.8

## 10.0.4

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/lockfile-file@8.1.3
  - @pnpm/core-loggers@9.0.3
  - @pnpm/modules-yaml@12.1.3
  - @pnpm/read-projects-context@8.0.7

## 10.0.3

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/lockfile-file@8.1.2
  - @pnpm/core-loggers@9.0.2
  - @pnpm/modules-yaml@12.1.2
  - @pnpm/read-projects-context@8.0.6

## 10.0.2

### Patch Changes

- Updated dependencies [302ebffc5]
  - @pnpm/constants@7.1.1
  - @pnpm/lockfile-file@8.1.1
  - @pnpm/error@5.0.2
  - @pnpm/read-projects-context@8.0.5

## 10.0.1

### Patch Changes

- 4b97f1f07: Don't use await in loops.

## 10.0.0

### Major Changes

- a53ef4d19: New property returned: `existsNonEmptyWantedLockfile`.
  The `existsWantedLockfile` now means only that a file existed.
- 9c4ae87bd: New required options added: autoInstallPeers and excludeLinksFromLockfile.

### Patch Changes

- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
- Updated dependencies [9c4ae87bd]
  - @pnpm/lockfile-file@8.1.0
  - @pnpm/types@9.1.0
  - @pnpm/constants@7.1.0
  - @pnpm/read-projects-context@8.0.4
  - @pnpm/core-loggers@9.0.1
  - @pnpm/modules-yaml@12.1.1
  - @pnpm/error@5.0.1

## 9.1.0

### Minor Changes

- 1ffedcb8d: New option added: confirmModulesPurge.

## 9.0.4

### Patch Changes

- 497b0a79c: Ask the user to confirm the removal of node_modules directory unless the `--force` option is passed.
- Updated dependencies [e6b83c84e]
  - @pnpm/modules-yaml@12.1.0
  - @pnpm/read-projects-context@8.0.3

## 9.0.3

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/lockfile-file@8.0.2
  - @pnpm/read-projects-context@8.0.2

## 9.0.2

### Patch Changes

- 080fee0b8: Add -g to mismatch registries error info when original command has -g option [#6224](https://github.com/pnpm/pnpm/issues/6224).

## 9.0.1

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
  - @pnpm/lockfile-file@8.0.1
  - @pnpm/read-projects-context@8.0.1

## 9.0.0

### Major Changes

- 158d8cf22: `useLockfileV6` field is deleted. Lockfile v5 cannot be written anymore, only transformed to the new format.
- eceaa8b8b: Node.js 14 support dropped.

### Minor Changes

- 2a2032810: Return `wantedLockfileIsModified`.

### Patch Changes

- Updated dependencies [158d8cf22]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [417c8ac59]
  - @pnpm/lockfile-file@8.0.0
  - @pnpm/read-projects-context@8.0.0
  - @pnpm/modules-yaml@12.0.0
  - @pnpm/core-loggers@9.0.0
  - @pnpm/constants@7.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0

## 8.2.4

### Patch Changes

- Updated dependencies [787c43dcc]
  - @pnpm/lockfile-file@7.0.6
  - @pnpm/read-projects-context@7.0.12

## 8.2.3

### Patch Changes

- Updated dependencies [ed946c73e]
  - @pnpm/lockfile-file@7.0.5
  - @pnpm/read-projects-context@7.0.11

## 8.2.2

### Patch Changes

- @pnpm/lockfile-file@7.0.4
- @pnpm/read-projects-context@7.0.10

## 8.2.1

### Patch Changes

- @pnpm/lockfile-file@7.0.3
- @pnpm/read-projects-context@7.0.9

## 8.2.0

### Minor Changes

- 28b47a156: When `extend-node-path` is set to `false`, the `NODE_PATH` environment variable is not set in the command shims [#5910](https://github.com/pnpm/pnpm/pull/5910)

## 8.1.2

### Patch Changes

- Updated dependencies [9a68ebbae]
  - @pnpm/lockfile-file@7.0.2
  - @pnpm/read-projects-context@7.0.8

## 8.1.1

### Patch Changes

- @pnpm/lockfile-file@7.0.1
- @pnpm/read-projects-context@7.0.7

## 8.1.0

### Minor Changes

- 3ebce5db7: Added support for `pnpm-lock.yaml` format v6. This new format will be the new lockfile format in pnpm v8. To use the new lockfile format, use the `use-lockfile-v6=true` setting in `.npmrc`. Or run `pnpm install --use-lockfile-v6` [#5810](https://github.com/pnpm/pnpm/pull/5810).

### Patch Changes

- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
  - @pnpm/constants@6.2.0
  - @pnpm/lockfile-file@7.0.0
  - @pnpm/error@4.0.1
  - @pnpm/read-projects-context@7.0.6

## 8.0.6

### Patch Changes

- 08ceaf3fc: replace dependency `is-ci` by `ci-info` (`is-ci` is just a simple wrapper around `ci-info`).

## 8.0.5

### Patch Changes

- Updated dependencies [b77651d14]
- Updated dependencies [2458741fa]
  - @pnpm/types@8.10.0
  - @pnpm/modules-yaml@11.1.0
  - @pnpm/lockfile-file@6.0.5
  - @pnpm/core-loggers@8.0.3
  - @pnpm/read-projects-context@7.0.5

## 8.0.4

### Patch Changes

- @pnpm/lockfile-file@6.0.4
- @pnpm/read-projects-context@7.0.4

## 8.0.3

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/lockfile-file@6.0.3
  - @pnpm/read-projects-context@7.0.3

## 8.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - @pnpm/core-loggers@8.0.2
  - @pnpm/lockfile-file@6.0.2
  - @pnpm/modules-yaml@11.0.2
  - @pnpm/read-projects-context@7.0.2

## 8.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/core-loggers@8.0.1
  - @pnpm/lockfile-file@6.0.1
  - @pnpm/modules-yaml@11.0.1
  - @pnpm/read-projects-context@7.0.1

## 8.0.0

### Major Changes

- 645384bfd: Breaking changes to the API.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [72f7d6b3b]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/modules-yaml@11.0.0
  - @pnpm/core-loggers@8.0.0
  - @pnpm/lockfile-file@6.0.0
  - @pnpm/read-projects-context@7.0.0

## 7.0.3

### Patch Changes

- Updated dependencies [7c296fe9b]
  - @pnpm/lockfile-file@5.3.8
  - @pnpm/read-projects-context@6.0.19

## 7.0.2

### Patch Changes

- Updated dependencies [3ae888c28]
  - @pnpm/core-loggers@7.1.0

## 7.0.1

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/lockfile-file@5.3.7
  - @pnpm/read-projects-context@6.0.18

## 7.0.0

### Major Changes

- 51566e34b: Pass readPackageHook as a separate option not as a subproperty of `hooks`.

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/core-loggers@7.0.8
  - @pnpm/lockfile-file@5.3.6
  - @pnpm/modules-yaml@10.0.8
  - @pnpm/read-projects-context@6.0.17

## 6.2.11

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/core-loggers@7.0.7
  - @pnpm/lockfile-file@5.3.5
  - @pnpm/modules-yaml@10.0.7
  - @pnpm/read-projects-context@6.0.16

## 6.2.10

### Patch Changes

- Updated dependencies [0373af22e]
  - @pnpm/lockfile-file@5.3.4
  - @pnpm/read-projects-context@6.0.15

## 6.2.9

### Patch Changes

- Updated dependencies [1e5482da4]
  - @pnpm/lockfile-file@5.3.3
  - @pnpm/read-projects-context@6.0.14

## 6.2.8

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [8103f92bd]
  - @pnpm/lockfile-file@5.3.2
  - @pnpm/read-projects-context@6.0.13

## 6.2.7

### Patch Changes

- Updated dependencies [44544b493]
- Updated dependencies [c90798461]
  - @pnpm/lockfile-file@5.3.1
  - @pnpm/types@8.5.0
  - @pnpm/read-projects-context@6.0.12
  - @pnpm/core-loggers@7.0.6
  - @pnpm/modules-yaml@10.0.6

## 6.2.6

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-file@5.3.0
  - @pnpm/read-projects-context@6.0.11

## 6.2.5

### Patch Changes

- Updated dependencies [4fa1091c8]
  - @pnpm/lockfile-file@5.2.0
  - @pnpm/read-projects-context@6.0.10

## 6.2.4

### Patch Changes

- Updated dependencies [ab684d77e]
  - @pnpm/lockfile-file@5.1.4
  - @pnpm/read-projects-context@6.0.9

## 6.2.3

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/lockfile-file@5.1.3
  - @pnpm/read-projects-context@6.0.8

## 6.2.2

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/lockfile-file@5.1.2
  - @pnpm/core-loggers@7.0.5
  - @pnpm/modules-yaml@10.0.5
  - @pnpm/read-projects-context@6.0.7

## 6.2.1

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/core-loggers@7.0.4
  - @pnpm/lockfile-file@5.1.1
  - @pnpm/modules-yaml@10.0.4
  - @pnpm/read-projects-context@6.0.6

## 6.2.0

### Minor Changes

- 56cf04cb3: New settings added: use-git-branch-lockfile, merge-git-branch-lockfiles, merge-git-branch-lockfiles-branch-pattern.

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [56cf04cb3]
  - @pnpm/types@8.2.0
  - @pnpm/lockfile-file@5.1.0
  - @pnpm/core-loggers@7.0.3
  - @pnpm/modules-yaml@10.0.3
  - @pnpm/read-projects-context@6.0.5

## 6.1.3

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/core-loggers@7.0.2
  - @pnpm/lockfile-file@5.0.4
  - @pnpm/modules-yaml@10.0.2
  - @pnpm/read-projects-context@6.0.4

## 6.1.2

### Patch Changes

- Updated dependencies [52b0576af]
  - @pnpm/lockfile-file@5.0.3
  - @pnpm/read-projects-context@6.0.3

## 6.1.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/core-loggers@7.0.1
  - @pnpm/lockfile-file@5.0.2
  - @pnpm/modules-yaml@10.0.1
  - @pnpm/read-projects-context@6.0.2

## 6.1.0

### Minor Changes

- 8fa95fd86: `extraNodePaths` added to the context.

### Patch Changes

- Updated dependencies [1267e4eff]
  - @pnpm/constants@6.1.0
  - @pnpm/error@3.0.1
  - @pnpm/lockfile-file@5.0.1
  - @pnpm/read-projects-context@6.0.1

## 6.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/error@3.0.0
  - @pnpm/lockfile-file@5.0.0
  - @pnpm/modules-yaml@10.0.0
  - @pnpm/read-projects-context@6.0.0

## 5.3.8

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/lockfile-file@4.3.1
  - @pnpm/read-projects-context@5.0.19

## 5.3.7

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-file@4.3.0
  - @pnpm/types@7.10.0
  - @pnpm/read-projects-context@5.0.18
  - @pnpm/core-loggers@6.1.4
  - @pnpm/modules-yaml@9.1.1

## 5.3.6

### Patch Changes

- Updated dependencies [cdc521cfa]
  - @pnpm/modules-yaml@9.1.0
  - @pnpm/read-projects-context@5.0.17

## 5.3.5

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/core-loggers@6.1.3
  - @pnpm/lockfile-file@4.2.6
  - @pnpm/modules-yaml@9.0.11
  - @pnpm/read-projects-context@5.0.16

## 5.3.4

### Patch Changes

- Updated dependencies [7375396db]
  - @pnpm/modules-yaml@9.0.10
  - @pnpm/read-projects-context@5.0.15

## 5.3.3

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/core-loggers@6.1.2
  - @pnpm/lockfile-file@4.2.5
  - @pnpm/modules-yaml@9.0.9
  - @pnpm/read-projects-context@5.0.14

## 5.3.2

### Patch Changes

- Updated dependencies [eb9ebd0f3]
- Updated dependencies [eb9ebd0f3]
  - @pnpm/lockfile-file@4.2.4
  - @pnpm/read-projects-context@5.0.13

## 5.3.1

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/core-loggers@6.1.1
  - @pnpm/lockfile-file@4.2.3
  - @pnpm/modules-yaml@9.0.8
  - @pnpm/read-projects-context@5.0.12

## 5.3.0

### Minor Changes

- 25f0fa9fa: Export `GetContextOptions`.

### Patch Changes

- Updated dependencies [ba9b2eba1]
- Updated dependencies [ba9b2eba1]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/types@7.7.0
  - @pnpm/read-projects-context@5.0.11
  - @pnpm/lockfile-file@4.2.2
  - @pnpm/modules-yaml@9.0.7

## 5.2.2

### Patch Changes

- @pnpm/read-projects-context@5.0.10

## 5.2.1

### Patch Changes

- @pnpm/read-projects-context@5.0.9

## 5.2.0

### Minor Changes

- 302ae4f6f: Support async hooks

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/core-loggers@6.0.6
  - @pnpm/lockfile-file@4.2.1
  - @pnpm/modules-yaml@9.0.6
  - @pnpm/read-projects-context@5.0.8

## 5.1.6

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/lockfile-file@4.2.0
  - @pnpm/core-loggers@6.0.5
  - @pnpm/modules-yaml@9.0.5
  - @pnpm/read-projects-context@5.0.7

## 5.1.5

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/core-loggers@6.0.4
  - @pnpm/lockfile-file@4.1.1
  - @pnpm/modules-yaml@9.0.4
  - @pnpm/read-projects-context@5.0.6

## 5.1.4

### Patch Changes

- Updated dependencies [8e76690f4]
- Updated dependencies [8e76690f4]
  - @pnpm/lockfile-file@4.1.0
  - @pnpm/types@7.3.0
  - @pnpm/read-projects-context@5.0.5
  - @pnpm/core-loggers@6.0.3
  - @pnpm/modules-yaml@9.0.3

## 5.1.3

### Patch Changes

- Updated dependencies [2dc5a7a4c]
  - @pnpm/lockfile-file@4.0.4
  - @pnpm/read-projects-context@5.0.4

## 5.1.2

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/core-loggers@6.0.2
  - @pnpm/lockfile-file@4.0.3
  - @pnpm/modules-yaml@9.0.2
  - @pnpm/read-projects-context@5.0.3

## 5.1.1

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/lockfile-file@4.0.2
  - @pnpm/read-projects-context@5.0.2

## 5.1.0

### Minor Changes

- 97c64bae4: Pass in the location of the project to the `readPackage` hook.

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/core-loggers@6.0.1
  - @pnpm/lockfile-file@4.0.1
  - @pnpm/modules-yaml@9.0.1
  - @pnpm/read-projects-context@5.0.1

## 5.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 7adc6e875: Update dependencies.
- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [90487a3a8]
- Updated dependencies [155e70597]
- Updated dependencies [9c2a878c3]
- Updated dependencies [8b66f26dc]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [f7750baed]
- Updated dependencies [78470a32d]
- Updated dependencies [9c2a878c3]
  - @pnpm/constants@5.0.0
  - @pnpm/core-loggers@6.0.0
  - @pnpm/error@2.0.0
  - @pnpm/lockfile-file@4.0.0
  - @pnpm/modules-yaml@9.0.0
  - @pnpm/read-projects-context@5.0.0
  - @pnpm/types@7.0.0

## 4.0.0

### Major Changes

- 51e1456dd: `opts.autofixMergeConflicts` is replaced with `opts.frozenLockfile`.

  When `opts.frozenLockfile` is `false`, broken lockfiles are ignored and merge conflicts are automatically resolved.

### Patch Changes

- Updated dependencies [51e1456dd]
  - @pnpm/lockfile-file@3.2.1
  - @pnpm/read-projects-context@4.0.16

## 3.3.6

### Patch Changes

- 27a40321c: Update dependencies.

## 3.3.5

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-file@3.2.0
  - @pnpm/types@6.4.0
  - @pnpm/read-projects-context@4.0.15
  - @pnpm/core-loggers@5.0.3
  - @pnpm/modules-yaml@8.0.6

## 3.3.4

### Patch Changes

- Updated dependencies [af897c324]
  - @pnpm/lockfile-file@3.1.4
  - @pnpm/read-projects-context@4.0.14

## 3.3.3

### Patch Changes

- Updated dependencies [1e4a3a17a]
  - @pnpm/lockfile-file@3.1.3
  - @pnpm/read-projects-context@4.0.13

## 3.3.2

### Patch Changes

- Updated dependencies [fba715512]
  - @pnpm/lockfile-file@3.1.2
  - @pnpm/read-projects-context@4.0.12

## 3.3.1

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/lockfile-file@3.1.1
  - @pnpm/read-projects-context@4.0.11

## 3.3.0

### Minor Changes

- 3776b5a52: A new option added to the context: lockfileHadConflicts.

### Patch Changes

- Updated dependencies [3776b5a52]
  - @pnpm/lockfile-file@3.1.0
  - @pnpm/read-projects-context@4.0.10

## 3.2.11

### Patch Changes

- Updated dependencies [dbcc6c96f]
- Updated dependencies [09492b7b4]
  - @pnpm/lockfile-file@3.0.18
  - @pnpm/modules-yaml@8.0.5
  - @pnpm/read-projects-context@4.0.9

## 3.2.10

### Patch Changes

- Updated dependencies [aa6bc4f95]
  - @pnpm/lockfile-file@3.0.17
  - @pnpm/read-projects-context@4.0.8

## 3.2.9

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/lockfile-file@3.0.16
  - @pnpm/core-loggers@5.0.2
  - @pnpm/modules-yaml@8.0.4
  - @pnpm/read-projects-context@4.0.7

## 3.2.8

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - @pnpm/lockfile-file@3.0.15
  - @pnpm/core-loggers@5.0.1
  - @pnpm/modules-yaml@8.0.3
  - @pnpm/read-projects-context@4.0.6

## 3.2.7

### Patch Changes

- ac3042858: When purging an incompatible modules directory, don't remove `.dot_files` that don't belong to pnpm. (<https://github.com/pnpm/pnpm/issues/2506>)

## 3.2.6

### Patch Changes

- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0

## 3.2.5

### Patch Changes

- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/lockfile-file@3.0.14
  - @pnpm/read-projects-context@4.0.5

## 3.2.4

### Patch Changes

- 972864e0d: publicHoistPattern=undefined should be considered to be the same as publicHoistPattern='' (empty string).
- Updated dependencies [9550b0505]
  - @pnpm/lockfile-file@3.0.13
  - @pnpm/read-projects-context@4.0.4

## 3.2.3

### Patch Changes

- 51086e6e4: Fix text in registries mismatch error message.
- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/lockfile-file@3.0.12
  - @pnpm/read-projects-context@4.0.3

## 3.2.2

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - @pnpm/modules-yaml@8.0.2
  - @pnpm/read-projects-context@4.0.2

## 3.2.1

### Patch Changes

- 25b425ca2: When purging an incompatible modules directory, don't remove the actual directory, just the contents of it.

## 3.2.0

### Minor Changes

- a01626668: Add `originalManifest` that stores the unmodified.

## 3.1.0

### Minor Changes

- 9a908bc07: Use `contextLogger` to log `virtualStoreDir`, `storeDir`, and `currentLockfileExists`.

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0

## 3.0.1

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - @pnpm/lockfile-file@3.0.11
  - @pnpm/modules-yaml@8.0.1
  - @pnpm/read-projects-context@4.0.1

## 3.0.0

### Major Changes

- 71a8c8ce3: `hoistedAliases` replaced with `hoistedDependencies`.

  `shamefullyHoist` replaced with `publicHoistPattern`.

  `forceShamefullyHoist` replaced with `forcePublicHoistPattern`.

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/read-projects-context@4.0.0
  - @pnpm/types@6.1.0
  - @pnpm/modules-yaml@8.0.0
  - @pnpm/core-loggers@4.1.1
  - @pnpm/lockfile-file@3.0.10

## 2.1.2

### Patch Changes

- Updated dependencies [2ebb7af33]
  - @pnpm/core-loggers@4.1.0

## 2.1.1

### Patch Changes

- 58c02009f: When checking compatibility of the existing modules directory, start with the layout version. Otherwise, it may happen that some of the fields were renamed and other checks will fail.

## 2.1.0

### Minor Changes

- 327bfbf02: Add `currentLockfileIsUpToDate` to the context.

## 2.0.0

### Major Changes

- 3f73eaf0c: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- 802d145fc: Remove `independent-leaves` support.
- e3990787a: Rename NodeModules to Modules in option names.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [3f73eaf0c]
- Updated dependencies [da091c711]
- Updated dependencies [802d145fc]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/modules-yaml@7.0.0
  - @pnpm/types@6.0.0
  - @pnpm/read-projects-context@3.0.0
  - @pnpm/core-loggers@4.0.2
  - @pnpm/error@1.2.1
  - @pnpm/lockfile-file@3.0.9

## 2.0.0-alpha.2

### Patch Changes

- Updated dependencies [ca9f50844]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/lockfile-file@3.0.9-alpha.2
  - @pnpm/read-projects-context@2.0.2-alpha.2

## 2.0.0-alpha.1

### Major Changes

- 3f73eaf0: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- e3990787: Rename NodeModules to Modules in option names.

### Patch Changes

- Updated dependencies [3f73eaf0]
- Updated dependencies [da091c71]
  - @pnpm/modules-yaml@7.0.0-alpha.0
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/read-projects-context@2.0.2-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.1

## 1.2.2-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.0
  - @pnpm/read-projects-context@2.0.2-alpha.0

## 1.2.1

### Patch Changes

- 907c63a48: Update dependencies.
- 907c63a48: Use `fs.mkdir` instead of `make-dir`.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/lockfile-file@3.0.8
  - @pnpm/modules-yaml@6.0.2
  - @pnpm/read-projects-context@2.0.1
