# @pnpm/plugin-commands-publishing

## 4.2.10

### Patch Changes

- Updated dependencies [6681fdcbc]
  - @pnpm/config@12.5.0
  - @pnpm/cli-utils@0.6.21
  - @pnpm/client@5.0.5

## 4.2.9

### Patch Changes

- Updated dependencies [97f90e537]
  - @pnpm/package-bins@5.0.5
  - @pnpm/client@5.0.4
  - @pnpm/cli-utils@0.6.20

## 4.2.8

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9
  - @pnpm/cli-utils@0.6.19

## 4.2.7

### Patch Changes

- @pnpm/config@12.4.8
- @pnpm/cli-utils@0.6.18

## 4.2.6

### Patch Changes

- @pnpm/client@5.0.3

## 4.2.5

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7
  - @pnpm/cli-utils@0.6.17

## 4.2.4

### Patch Changes

- @pnpm/client@5.0.2

## 4.2.3

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6
  - @pnpm/cli-utils@0.6.16

## 4.2.2

### Patch Changes

- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5
  - @pnpm/cli-utils@0.6.15

## 4.2.1

### Patch Changes

- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4
  - @pnpm/cli-utils@0.6.14

## 4.2.0

### Minor Changes

- b734b45ea: By default, for portability reasons, no files except those listed in the bin field will be marked as executable in the resulting package archive. The executableFiles field lets you declare additional fields that must have the executable flag (+x) set even if they aren't directly accessible through the bin field.

  ```json
  "publishConfig": {
    "executableFiles": [
      "./dist/shim.js",
    ]
  }
  ```

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/cli-utils@0.6.13
  - @pnpm/config@12.4.3
  - @pnpm/exportable-manifest@2.1.6
  - @pnpm/lifecycle@11.0.4
  - @pnpm/package-bins@5.0.4
  - @pnpm/pick-registry-for-package@2.0.4
  - @pnpm/resolver-base@8.0.4
  - @pnpm/sort-packages@2.1.1
  - @pnpm/client@5.0.1

## 4.1.3

### Patch Changes

- 47ed7b163: Scripts should be executed upon the original package.json, when publishConfig.directory is set.

## 4.1.2

### Patch Changes

- f9152ab3c: Fix the help description of the pack command.
- Updated dependencies [7af16a011]
- Updated dependencies [73c1f802e]
  - @pnpm/lifecycle@11.0.3
  - @pnpm/config@12.4.2
  - @pnpm/cli-utils@0.6.12

## 4.1.1

### Patch Changes

- efca3896c: Use the correct compression algorithm to pack.

## 4.1.0

### Minor Changes

- f63c034c6: `pnpm pack` uses its own inhouse implementation. `pnpm pack` is not using `npm pack`.
- f63c034c6: Run prepublish and prepublishOnly before packing a package.

### Patch Changes

- f63c034c6: Do not modify the package.json file before packing the package. Do not copy LICENSE files from the root of the workspace (the files are still packed).
  - @pnpm/cli-utils@0.6.11

## 4.0.1

### Patch Changes

- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1
  - @pnpm/cli-utils@0.6.10

## 4.0.0

### Major Changes

- 691f64713: New required option added: cacheDir.

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [691f64713]
- Updated dependencies [691f64713]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0
  - @pnpm/client@5.0.0
  - @pnpm/cli-utils@0.6.9

## 3.3.4

### Patch Changes

- Updated dependencies [1442f8786]
- Updated dependencies [8e76690f4]
  - @pnpm/sort-packages@2.1.0
  - @pnpm/types@7.3.0
  - @pnpm/cli-utils@0.6.8
  - @pnpm/config@12.3.3
  - @pnpm/exportable-manifest@2.1.5
  - @pnpm/lifecycle@11.0.2
  - @pnpm/pick-registry-for-package@2.0.3
  - @pnpm/resolver-base@8.0.3
  - @pnpm/client@4.0.2

## 3.3.3

### Patch Changes

- 40ce0eb6b: Copy the `.npmrc` from the root of the repository.

## 3.3.2

### Patch Changes

- @pnpm/client@4.0.1

## 3.3.1

### Patch Changes

- b5e9284c3: fix publishConfig.directory script

## 3.3.0

### Minor Changes

- 724c5abd8: support "publishConfig.directory" field

### Patch Changes

- Updated dependencies [eeff424bd]
- Updated dependencies [724c5abd8]
  - @pnpm/client@4.0.0
  - @pnpm/run-npm@3.1.0
  - @pnpm/types@7.2.0
  - @pnpm/cli-utils@0.6.7
  - @pnpm/config@12.3.2
  - @pnpm/exportable-manifest@2.1.4
  - @pnpm/lifecycle@11.0.1
  - @pnpm/pick-registry-for-package@2.0.2
  - @pnpm/resolver-base@8.0.2
  - @pnpm/sort-packages@2.0.2

## 3.2.2

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1
  - @pnpm/exportable-manifest@2.1.3
  - @pnpm/cli-utils@0.6.6
  - @pnpm/client@3.1.6

## 3.2.1

### Patch Changes

- @pnpm/client@3.1.5

## 3.2.0

### Minor Changes

- 819f67894: New option: reportSummary. When it is set to `true`, recursive publish will save the summary of published packages to `pnpm-publish-summary.json`.

### Patch Changes

- Updated dependencies [84ec82e05]
- Updated dependencies [c2a71e4fd]
- Updated dependencies [84ec82e05]
  - @pnpm/config@12.3.0
  - @pnpm/cli-utils@0.6.5

## 3.1.5

### Patch Changes

- Updated dependencies [6a1468495]
  - @pnpm/exportable-manifest@2.1.2

## 3.1.4

### Patch Changes

- @pnpm/cli-utils@0.6.4
- @pnpm/client@3.1.4

## 3.1.3

### Patch Changes

- @pnpm/cli-utils@0.6.3
- @pnpm/exportable-manifest@2.1.1
- @pnpm/client@3.1.3
- @pnpm/config@12.2.0

## 3.1.2

### Patch Changes

- @pnpm/client@3.1.2

## 3.1.1

### Patch Changes

- Updated dependencies [e6a2654a2]
  - @pnpm/lifecycle@11.0.0
  - @pnpm/client@3.1.1
  - @pnpm/config@12.2.0

## 3.1.0

### Minor Changes

- 05baaa6e7: Add new config setting: `fetch-timeout`.
- 85fb21a83: Add support for workspace:^ and workspace:~ aliases

### Patch Changes

- Updated dependencies [05baaa6e7]
- Updated dependencies [dfdf669e6]
- Updated dependencies [85fb21a83]
- Updated dependencies [05baaa6e7]
- Updated dependencies [97c64bae4]
  - @pnpm/config@12.2.0
  - @pnpm/exportable-manifest@2.1.0
  - @pnpm/client@3.1.0
  - @pnpm/types@7.1.0
  - @pnpm/cli-utils@0.6.2
  - @pnpm/lifecycle@10.0.1
  - @pnpm/pick-registry-for-package@2.0.1
  - @pnpm/resolver-base@8.0.1
  - @pnpm/sort-packages@2.0.1

## 3.0.2

### Patch Changes

- Updated dependencies [ba5231ccf]
  - @pnpm/config@12.1.0
  - @pnpm/cli-utils@0.6.1

## 3.0.1

### Patch Changes

- Updated dependencies [561276d2c]
  - @pnpm/exportable-manifest@2.0.1
  - @pnpm/client@3.0.1
  - @pnpm/config@12.0.0

## 3.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [78470a32d]
- Updated dependencies [aed712455]
- Updated dependencies [aed712455]
  - @pnpm/cli-utils@0.6.0
  - @pnpm/client@3.0.0
  - @pnpm/config@12.0.0
  - @pnpm/error@2.0.0
  - @pnpm/exportable-manifest@2.0.0
  - @pnpm/lifecycle@10.0.0
  - @pnpm/pick-registry-for-package@2.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/run-npm@3.0.0
  - @pnpm/sort-packages@2.0.0
  - @pnpm/types@7.0.0

## 2.5.6

### Patch Changes

- Updated dependencies [4f1ce907a]
  - @pnpm/config@11.14.2
  - @pnpm/cli-utils@0.5.4

## 2.5.5

### Patch Changes

- Updated dependencies [d853fb14a]
- Updated dependencies [4b3852c39]
  - @pnpm/lifecycle@9.6.5
  - @pnpm/config@11.14.1
  - @pnpm/cli-utils@0.5.3

## 2.5.4

### Patch Changes

- @pnpm/client@2.0.24

## 2.5.3

### Patch Changes

- @pnpm/client@2.0.23
- @pnpm/config@11.14.0
- @pnpm/cli-utils@0.5.2

## 2.5.2

### Patch Changes

- Updated dependencies [3be2b1773]
  - @pnpm/cli-utils@0.5.1

## 2.5.1

### Patch Changes

- @pnpm/client@2.0.22

## 2.5.0

### Minor Changes

- `pnpm publish -r --force` publishes packages even if their current version is already in the registry.

## 2.4.3

### Patch Changes

- 249c068dd: add pref to pick registries
- Updated dependencies [cb040ae18]
- Updated dependencies [249c068dd]
  - @pnpm/cli-utils@0.5.0
  - @pnpm/config@11.14.0
  - @pnpm/pick-registry-for-package@1.1.0

## 2.4.2

### Patch Changes

- Updated dependencies [c4cc62506]
  - @pnpm/config@11.13.0
  - @pnpm/cli-utils@0.4.51
  - @pnpm/exportable-manifest@1.2.2
  - @pnpm/client@2.0.21

## 2.4.1

### Patch Changes

- cc39fc8ad: fix: remove --publish-branch with branch name to npm publish args
- Updated dependencies [bff84dbca]
  - @pnpm/config@11.12.1
  - @pnpm/cli-utils@0.4.50

## 2.4.0

### Minor Changes

- ff6129041: feat: print an info message when there's nothing new to publish recursively

### Patch Changes

- @pnpm/cli-utils@0.4.49

## 2.3.15

### Patch Changes

- @pnpm/cli-utils@0.4.48

## 2.3.14

### Patch Changes

- Updated dependencies [9a9bc67d2]
  - @pnpm/lifecycle@9.6.4

## 2.3.13

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [548f28df9]
- Updated dependencies [548f28df9]
  - @pnpm/types@6.4.0
  - @pnpm/cli-utils@0.4.47
  - @pnpm/config@11.12.0
  - @pnpm/exportable-manifest@1.2.1
  - @pnpm/lifecycle@9.6.3
  - @pnpm/pick-registry-for-package@1.0.6
  - @pnpm/resolver-base@7.1.1
  - @pnpm/sort-packages@1.0.16
  - @pnpm/client@2.0.20

## 2.3.12

### Patch Changes

- @pnpm/config@11.11.1
- @pnpm/cli-utils@0.4.46

## 2.3.11

### Patch Changes

- Updated dependencies [c854f8547]
  - @pnpm/exportable-manifest@1.2.0

## 2.3.10

### Patch Changes

- 6af60416c: add 'main' to default publish branch

## 2.3.9

### Patch Changes

- Updated dependencies [f40bc5927]
  - @pnpm/config@11.11.0
  - @pnpm/cli-utils@0.4.45

## 2.3.8

### Patch Changes

- Updated dependencies [425c7547d]
  - @pnpm/config@11.10.2
  - @pnpm/cli-utils@0.4.44
  - @pnpm/client@2.0.19

## 2.3.7

### Patch Changes

- Updated dependencies [ea09da716]
  - @pnpm/config@11.10.1
  - @pnpm/cli-utils@0.4.43

## 2.3.6

### Patch Changes

- Updated dependencies [a8656b42f]
  - @pnpm/config@11.10.0
  - @pnpm/cli-utils@0.4.42

## 2.3.5

### Patch Changes

- Updated dependencies [041537bc3]
  - @pnpm/config@11.9.1
  - @pnpm/cli-utils@0.4.41

## 2.3.4

### Patch Changes

- @pnpm/client@2.0.18

## 2.3.3

### Patch Changes

- @pnpm/client@2.0.17

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
