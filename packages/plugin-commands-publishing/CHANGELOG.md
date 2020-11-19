# @pnpm/plugin-commands-publishing

## 2.3.2

### Patch Changes

- @pnpm/client@2.0.16

## 2.3.1

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0
  - @pnpm/resolver-base@7.1.0
  - @pnpm/cli-utils@0.4.40
  - @pnpm/client@2.0.15

## 2.3.0

### Minor Changes

- 084614f55: Support aliases to workspace packages. For instance, `"foo": "workspace:bar@*"` will link bar from the repository but aliased to foo. Before publish, these specs are converted to regular aliased versions.

### Patch Changes

- Updated dependencies [284e95c5e]
- Updated dependencies [084614f55]
- Updated dependencies [fcc1c7100]
  - @pnpm/exportable-manifest@1.1.0
  - @pnpm/config@11.8.0
  - @pnpm/cli-utils@0.4.39
  - @pnpm/client@2.0.14

## 2.2.16

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/client@2.0.13
  - @pnpm/cli-utils@0.4.38
  - @pnpm/config@11.7.2
  - @pnpm/exportable-manifest@1.0.8
  - @pnpm/lifecycle@9.6.2

## 2.2.15

### Patch Changes

- 09492b7b4: Update write-file-atomic to v3.
  - @pnpm/cli-utils@0.4.37
  - @pnpm/exportable-manifest@1.0.7
  - @pnpm/client@2.0.12

## 2.2.14

### Patch Changes

- @pnpm/client@2.0.11
- @pnpm/cli-utils@0.4.36
- @pnpm/exportable-manifest@1.0.6

## 2.2.13

### Patch Changes

- @pnpm/client@2.0.10

## 2.2.12

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/cli-utils@0.4.35
  - @pnpm/config@11.7.1
  - @pnpm/exportable-manifest@1.0.5
  - @pnpm/lifecycle@9.6.1
  - @pnpm/pick-registry-for-package@1.0.5
  - @pnpm/resolver-base@7.0.5
  - @pnpm/sort-packages@1.0.15
  - @pnpm/client@2.0.9

## 2.2.11

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0
  - @pnpm/lifecycle@9.6.0
  - @pnpm/cli-utils@0.4.34

## 2.2.10

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/cli-utils@0.4.33
  - @pnpm/config@11.6.1
  - @pnpm/exportable-manifest@1.0.4
  - @pnpm/lifecycle@9.5.1
  - @pnpm/pick-registry-for-package@1.0.4
  - @pnpm/resolver-base@7.0.4
  - @pnpm/sort-packages@1.0.14
  - @pnpm/client@2.0.8

## 2.2.9

### Patch Changes

- Updated dependencies [f591fdeeb]
- Updated dependencies [f591fdeeb]
- Updated dependencies [f591fdeeb]
- Updated dependencies [3a83db407]
  - @pnpm/config@11.6.0
  - @pnpm/lifecycle@9.5.0
  - @pnpm/client@2.0.7
  - @pnpm/cli-utils@0.4.32

## 2.2.8

### Patch Changes

- @pnpm/cli-utils@0.4.31
- @pnpm/exportable-manifest@1.0.3
- @pnpm/client@2.0.6

## 2.2.7

### Patch Changes

- 5351791f6: Added more info to the Git check error hint.

## 2.2.6

### Patch Changes

- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0
  - @pnpm/cli-utils@0.4.30

## 2.2.5

### Patch Changes

- Updated dependencies [203e65ac8]
  - @pnpm/lifecycle@9.4.0
  - @pnpm/client@2.0.5

## 2.2.4

### Patch Changes

- 892e2b155: The order of Git checks is changed. The branch is checked after the cleannes check.
- Updated dependencies [23cf3c88b]
  - @pnpm/config@11.4.0
  - @pnpm/lifecycle@9.3.0
  - @pnpm/cli-utils@0.4.29

## 2.2.3

### Patch Changes

- @pnpm/client@2.0.4

## 2.2.2

### Patch Changes

- Updated dependencies [767212f4e]
- Updated dependencies [092f8dd83]
  - @pnpm/config@11.3.0
  - @pnpm/cli-utils@0.4.28

## 2.2.1

### Patch Changes

- @pnpm/client@2.0.3
- @pnpm/lifecycle@9.2.5
- @pnpm/cli-utils@0.4.27

## 2.2.0

### Minor Changes

- 273f11af4: More information added to the Git check errors and prompt.

### Patch Changes

- @pnpm/client@2.0.2

## 2.1.21

### Patch Changes

- @pnpm/cli-utils@0.4.26

## 2.1.20

### Patch Changes

- Updated dependencies [75a36deba]
- Updated dependencies [9f1a29ff9]
  - @pnpm/error@1.3.1
  - @pnpm/config@11.2.7
  - @pnpm/cli-utils@0.4.25
  - @pnpm/exportable-manifest@1.0.2
  - @pnpm/client@2.0.1
  - @pnpm/lifecycle@9.2.4

## 2.1.19

### Patch Changes

- Updated dependencies [ac0d3e122]
  - @pnpm/config@11.2.6
  - @pnpm/cli-utils@0.4.24

## 2.1.18

### Patch Changes

- Updated dependencies [855f8b00a]
- Updated dependencies [972864e0d]
- Updated dependencies [a1cdae3dc]
  - @pnpm/client@2.0.0
  - @pnpm/config@11.2.5
  - @pnpm/lifecycle@9.2.3
  - @pnpm/cli-utils@0.4.23

## 2.1.17

### Patch Changes

- 69a675f41: `pnpm publish -r` should not publish packages with `pnpm-temp` distribution tag.
- Updated dependencies [6d480dd7a]
- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/npm-resolver@9.1.0
  - @pnpm/cli-utils@0.4.22
  - @pnpm/config@11.2.4
  - @pnpm/exportable-manifest@1.0.1
  - @pnpm/fetch@2.1.3

## 2.1.16

### Patch Changes

- Updated dependencies [13c18e397]
  - @pnpm/config@11.2.3
  - @pnpm/cli-utils@0.4.21

## 2.1.15

### Patch Changes

- Updated dependencies [3f6d35997]
  - @pnpm/config@11.2.2
  - @pnpm/cli-utils@0.4.20

## 2.1.14

### Patch Changes

- Updated dependencies [edf1f412e]
  - @pnpm/exportable-manifest@1.0.0

## 2.1.13

### Patch Changes

- @pnpm/read-project-manifest@1.0.11
- @pnpm/cli-utils@0.4.19

## 2.1.12

### Patch Changes

- Updated dependencies [3bd3253e3]
  - @pnpm/read-project-manifest@1.0.10
  - @pnpm/cli-utils@0.4.18

## 2.1.11

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- 2a41ce95c: peerDependencies workspace substitution
- Updated dependencies [622c0b6f9]
- Updated dependencies [a2ef8084f]
  - @pnpm/npm-resolver@9.0.2
  - @pnpm/config@11.2.1
  - @pnpm/lifecycle@9.2.2
  - @pnpm/run-npm@2.0.3
  - @pnpm/cli-utils@0.4.17

## 2.1.10

### Patch Changes

- d44ff97f8: `pnpm publish -r --dry-run` should not publish anything to the registry.

## 2.1.9

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/cli-utils@0.4.16
  - @pnpm/config@11.2.0

## 2.1.8

### Patch Changes

- @pnpm/fetch@2.1.2
- @pnpm/lifecycle@9.2.1
- @pnpm/npm-resolver@9.0.1
- @pnpm/cli-utils@0.4.15

## 2.1.7

### Patch Changes

- 7b98d16c8: Update lru-cache to v6
- Updated dependencies [379cdcaf8]
- Updated dependencies [65b4d07ca]
- Updated dependencies [ab3b8f51d]
  - @pnpm/npm-resolver@9.0.1
  - @pnpm/config@11.1.0
  - @pnpm/cli-utils@0.4.14
  - @pnpm/fetch@2.1.1

## 2.1.6

### Patch Changes

- Updated dependencies [76aaead32]
  - @pnpm/lifecycle@9.2.0

## 2.1.5

### Patch Changes

- @pnpm/config@11.0.1
- @pnpm/cli-utils@0.4.13

## 2.1.4

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [71aeb9a38]
- Updated dependencies [71aeb9a38]
- Updated dependencies [915828b46]
  - @pnpm/config@11.0.0
  - @pnpm/fetch@2.1.0
  - @pnpm/npm-resolver@9.0.0
  - @pnpm/cli-utils@0.4.12

## 2.1.3

### Patch Changes

- @pnpm/config@10.0.1
- @pnpm/cli-utils@0.4.11

## 2.1.2

### Patch Changes

- Updated dependencies [db17f6f7b]
- Updated dependencies [1146b76d2]
- Updated dependencies [db17f6f7b]
  - @pnpm/config@10.0.0
  - @pnpm/types@6.2.0
  - @pnpm/cli-utils@0.4.10
  - @pnpm/lifecycle@9.1.3
  - @pnpm/npm-resolver@8.1.2
  - @pnpm/pick-registry-for-package@1.0.3
  - @pnpm/read-project-manifest@1.0.9
  - @pnpm/resolver-base@7.0.3
  - @pnpm/sort-packages@1.0.13

## 2.1.1

### Patch Changes

- 1520e3d6f: Update fast-glob to v3.2.4

## 2.1.0

### Minor Changes

- 6808c43fa: Don't request the full metadata just for getting the list of published versions.

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/config@9.2.0
  - @pnpm/cli-utils@0.4.9
  - @pnpm/lifecycle@9.1.2
  - @pnpm/npm-resolver@8.1.1
  - @pnpm/pick-registry-for-package@1.0.2
  - @pnpm/read-project-manifest@1.0.8
  - @pnpm/resolver-base@7.0.2
  - @pnpm/sort-packages@1.0.12

## 2.0.4

### Patch Changes

- Updated dependencies [57c510f00]
- Updated dependencies [e934b1a48]
  - @pnpm/read-project-manifest@1.0.7
  - @pnpm/cli-utils@0.4.8

## 2.0.3

### Patch Changes

- Updated dependencies [4cf7ef367]
- Updated dependencies [d3ddd023c]
- Updated dependencies [68d8dc68f]
  - @pnpm/npm-resolver@8.1.0
  - @pnpm/lifecycle@9.1.1
  - @pnpm/cli-utils@0.4.7

## 2.0.2

### Patch Changes

- @pnpm/npm-resolver@8.0.1

## 2.0.1

### Patch Changes

- Updated dependencies [c56438567]
- Updated dependencies [ffddf34a8]
- Updated dependencies [8094b2a62]
  - @pnpm/run-npm@2.0.2
  - @pnpm/config@9.1.0
  - @pnpm/lifecycle@9.1.0
  - @pnpm/cli-utils@0.4.6
  - @pnpm/sort-packages@1.0.11

## 2.0.0

### Major Changes

- 4063f1bee: Git checks are on by default.

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [5bc033c43]
- Updated dependencies [da091c711]
- Updated dependencies [f35a3ec1c]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [45fdcfde2]
- Updated dependencies [f453a5f46]
- Updated dependencies [e3990787a]
  - @pnpm/config@9.0.0
  - @pnpm/npm-resolver@8.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lifecycle@9.0.0
  - @pnpm/cli-utils@0.4.5
  - @pnpm/error@1.2.1
  - @pnpm/pick-registry-for-package@1.0.1
  - @pnpm/read-project-manifest@1.0.6
  - @pnpm/resolver-base@7.0.1
  - @pnpm/run-npm@2.0.2
  - @pnpm/sort-packages@1.0.10

## 2.0.0-alpha.4

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [45fdcfde2]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/cli-utils@0.4.5-alpha.2
  - @pnpm/sort-packages@1.0.10-alpha.2

## 2.0.0-alpha.3

### Patch Changes

- Updated dependencies [da091c71]
- Updated dependencies [e3990787]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/lifecycle@9.0.0-alpha.1
  - @pnpm/cli-utils@0.4.5-alpha.1
  - @pnpm/config@8.3.1-alpha.1
  - @pnpm/npm-resolver@7.3.12-alpha.2
  - @pnpm/pick-registry-for-package@1.0.1-alpha.0
  - @pnpm/read-project-manifest@1.0.6-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0
  - @pnpm/sort-packages@1.0.10-alpha.1

## 2.0.0-alpha.2

### Patch Changes

- Updated dependencies [5bc033c43]
  - @pnpm/npm-resolver@8.0.0-alpha.1
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/cli-utils@0.4.5-alpha.0
  - @pnpm/sort-packages@1.0.10-alpha.0

## 1.0.12-alpha.1

### Patch Changes

- Updated dependencies [f35a3ec1c]
- Updated dependencies [f453a5f46]
  - @pnpm/lifecycle@8.2.0-alpha.0
  - @pnpm/npm-resolver@7.3.12-alpha.0

## 2.0.0-alpha.0

### Major Changes

- 4063f1bee: Git checks are on by default.

## 1.0.12

### Patch Changes

- Updated dependencies [2ec4c4eb9]
  - @pnpm/lifecycle@8.2.0

## 1.0.11

### Patch Changes

- 907c63a48: Update `@pnpm/store-path`.
- 907c63a48: Dependencies updated.
  - @pnpm/read-project-manifest@1.0.5
  - @pnpm/cli-utils@0.4.4
