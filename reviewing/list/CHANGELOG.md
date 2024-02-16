# @pnpm/list

## 9.1.10

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@2.1.9

## 9.1.9

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@2.1.8

## 9.1.8

### Patch Changes

- 09f610349: `pnpm list --parseable` should not print the same dependency multiple times [#7429](https://github.com/pnpm/pnpm/issues/7429).
- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2
  - @pnpm/read-package-json@8.0.7
  - @pnpm/read-project-manifest@5.0.10
  - @pnpm/reviewing.dependencies-hierarchy@2.1.7

## 9.1.7

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/read-package-json@8.0.6
  - @pnpm/read-project-manifest@5.0.9
  - @pnpm/reviewing.dependencies-hierarchy@2.1.6

## 9.1.6

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@2.1.5

## 9.1.5

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@2.1.4

## 9.1.4

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@2.1.3

## 9.1.3

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/read-package-json@8.0.5
  - @pnpm/read-project-manifest@5.0.8
  - @pnpm/reviewing.dependencies-hierarchy@2.1.2

## 9.1.2

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/read-package-json@8.0.4
  - @pnpm/read-project-manifest@5.0.7
  - @pnpm/reviewing.dependencies-hierarchy@2.1.1

## 9.1.1

### Patch Changes

- 40798fb1c: Fix memory error in `pnpm why` when the dependencies tree is too big, the command will now prune the tree to just 10 end leafs and now supports `--depth` argument.

## 9.1.0

### Minor Changes

- 101c97ecb: Export the renderer functions.

### Patch Changes

- Updated dependencies [101c97ecb]
  - @pnpm/reviewing.dependencies-hierarchy@2.1.0

## 9.0.12

### Patch Changes

- @pnpm/read-project-manifest@5.0.6
- @pnpm/reviewing.dependencies-hierarchy@2.0.11

## 9.0.11

### Patch Changes

- f73eeac06: Don't fail when no `package.json` is found.
  - @pnpm/read-project-manifest@5.0.5
  - @pnpm/reviewing.dependencies-hierarchy@2.0.11

## 9.0.10

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/read-package-json@8.0.3
  - @pnpm/read-project-manifest@5.0.4
  - @pnpm/reviewing.dependencies-hierarchy@2.0.10

## 9.0.9

### Patch Changes

- Updated dependencies [b4892acc5]
- Updated dependencies [e334e5670]
  - @pnpm/read-project-manifest@5.0.3
  - @pnpm/reviewing.dependencies-hierarchy@2.0.9

## 9.0.8

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@2.0.8
- @pnpm/read-package-json@8.0.2
- @pnpm/read-project-manifest@5.0.2

## 9.0.7

### Patch Changes

- 4b97f1f07: Don't use await in loops.
  - @pnpm/reviewing.dependencies-hierarchy@2.0.7

## 9.0.6

### Patch Changes

- Updated dependencies [a9e0b7cbf]
  - @pnpm/types@9.1.0
  - @pnpm/reviewing.dependencies-hierarchy@2.0.6
  - @pnpm/read-package-json@8.0.1
  - @pnpm/read-project-manifest@5.0.1

## 9.0.5

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@2.0.5

## 9.0.4

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@2.0.4

## 9.0.3

### Patch Changes

- c0760128d: bump semver to 7.4.0
  - @pnpm/reviewing.dependencies-hierarchy@2.0.3

## 9.0.2

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@2.0.2

## 9.0.1

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@2.0.1

## 9.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/read-project-manifest@5.0.0
  - @pnpm/reviewing.dependencies-hierarchy@2.0.0
  - @pnpm/read-package-json@8.0.0
  - @pnpm/matcher@5.0.0
  - @pnpm/types@9.0.0

## 8.2.2

### Patch Changes

- 185ab01ad: When patch package does not specify a version, use locally installed version by default [#6192](https://github.com/pnpm/pnpm/issues/6192).
  - @pnpm/read-project-manifest@4.1.4
  - @pnpm/reviewing.dependencies-hierarchy@1.2.5

## 8.2.1

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@1.2.4

## 8.2.0

### Minor Changes

- b9ab2e0bf: Show path info for `pnpm why --json` or `--long` [#6103](https://github.com/pnpm/pnpm/issues/6103).

## 8.1.3

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@1.2.3

## 8.1.2

### Patch Changes

- 19e823bea: Show correct path info for dependenciesHierarchy tree
- Updated dependencies [19e823bea]
  - @pnpm/reviewing.dependencies-hierarchy@1.2.2

## 8.1.1

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@1.2.1

## 8.1.0

### Minor Changes

- 94ef3299e: Show dependency paths info in `pnpm audit` output [#3073](https://github.com/pnpm/pnpm/issues/3073)

### Patch Changes

- Updated dependencies [94ef3299e]
  - @pnpm/reviewing.dependencies-hierarchy@1.2.0

## 8.0.13

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@1.1.3

## 8.0.12

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@1.1.2

## 8.0.11

### Patch Changes

- @pnpm/reviewing.dependencies-hierarchy@1.1.1
- @pnpm/read-package-json@7.0.5
- @pnpm/read-project-manifest@4.1.3

## 8.0.10

### Patch Changes

- Updated dependencies [7853a26e1]
- Updated dependencies [395a33a50]
- Updated dependencies [395a33a50]
  - @pnpm/reviewing.dependencies-hierarchy@1.1.0

## 8.0.9

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/read-package-json@7.0.4
  - @pnpm/read-project-manifest@4.1.2
  - @pnpm/reviewing.dependencies-hierarchy@1.0.1

## 8.0.8

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/reviewing.dependencies-hierarchy@1.0.0

## 8.0.7

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/read-package-json@7.0.3
  - dependencies-hierarchy@12.0.4
  - @pnpm/read-project-manifest@4.1.1

## 8.0.6

### Patch Changes

- Updated dependencies [fec9e3149]
- Updated dependencies [0d12d38fd]
  - @pnpm/read-project-manifest@4.1.0

## 8.0.5

### Patch Changes

- Updated dependencies [969f8a002]
  - @pnpm/matcher@4.0.1

## 8.0.4

### Patch Changes

- dependencies-hierarchy@12.0.3

## 8.0.3

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - dependencies-hierarchy@12.0.2
  - @pnpm/read-package-json@7.0.2
  - @pnpm/read-project-manifest@4.0.2

## 8.0.2

### Patch Changes

- f36549165: `pnpm list --long --json` should print licenses and authors of packagese [#5533](https://github.com/pnpm/pnpm/pull/5533).

## 8.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - dependencies-hierarchy@12.0.1
  - @pnpm/read-package-json@7.0.1
  - @pnpm/read-project-manifest@4.0.1

## 8.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - dependencies-hierarchy@12.0.0
  - @pnpm/matcher@4.0.0
  - @pnpm/read-package-json@7.0.0
  - @pnpm/read-project-manifest@4.0.0

## 7.0.26

### Patch Changes

- dependencies-hierarchy@11.0.26
- @pnpm/read-project-manifest@3.0.13

## 7.0.25

### Patch Changes

- @pnpm/read-package-json@6.0.11
- @pnpm/read-project-manifest@3.0.12
- dependencies-hierarchy@11.0.25

## 7.0.24

### Patch Changes

- Updated dependencies [abb41a626]
- Updated dependencies [d665f3ff7]
  - @pnpm/matcher@3.2.0
  - @pnpm/types@8.7.0
  - dependencies-hierarchy@11.0.24
  - @pnpm/read-package-json@6.0.10
  - @pnpm/read-project-manifest@3.0.11

## 7.0.23

### Patch Changes

- Updated dependencies [156cc1ef6]
- Updated dependencies [9b44d38a4]
  - @pnpm/types@8.6.0
  - @pnpm/matcher@3.1.0
  - dependencies-hierarchy@11.0.23
  - @pnpm/read-package-json@6.0.9
  - @pnpm/read-project-manifest@3.0.10

## 7.0.22

### Patch Changes

- dependencies-hierarchy@11.0.22

## 7.0.21

### Patch Changes

- Updated dependencies [07bc24ad1]
  - @pnpm/read-package-json@6.0.8
  - dependencies-hierarchy@11.0.21

## 7.0.20

### Patch Changes

- dependencies-hierarchy@11.0.20

## 7.0.19

### Patch Changes

- dependencies-hierarchy@11.0.19

## 7.0.18

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [39c040127]
  - @pnpm/read-project-manifest@3.0.9
  - dependencies-hierarchy@11.0.18

## 7.0.17

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - dependencies-hierarchy@11.0.17
  - @pnpm/read-package-json@6.0.7
  - @pnpm/read-project-manifest@3.0.8

## 7.0.16

### Patch Changes

- dependencies-hierarchy@11.0.16

## 7.0.15

### Patch Changes

- dependencies-hierarchy@11.0.15

## 7.0.14

### Patch Changes

- Updated dependencies [01c5834bf]
  - @pnpm/read-project-manifest@3.0.7
  - dependencies-hierarchy@11.0.14

## 7.0.13

### Patch Changes

- dependencies-hierarchy@11.0.13

## 7.0.12

### Patch Changes

- dependencies-hierarchy@11.0.12

## 7.0.11

### Patch Changes

- dependencies-hierarchy@11.0.11

## 7.0.10

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- 42c1ea1c0: Update validate-npm-package-name to v4.
  - dependencies-hierarchy@11.0.10

## 7.0.9

### Patch Changes

- dependencies-hierarchy@11.0.9

## 7.0.8

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - dependencies-hierarchy@11.0.8
  - @pnpm/read-package-json@6.0.6
  - @pnpm/read-project-manifest@3.0.6

## 7.0.7

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - dependencies-hierarchy@11.0.7
  - @pnpm/read-package-json@6.0.5
  - @pnpm/read-project-manifest@3.0.5

## 7.0.6

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0
  - dependencies-hierarchy@11.0.6
  - @pnpm/read-package-json@6.0.4
  - @pnpm/read-project-manifest@3.0.4

## 7.0.5

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - dependencies-hierarchy@11.0.5
  - @pnpm/read-package-json@6.0.3
  - @pnpm/read-project-manifest@3.0.3

## 7.0.4

### Patch Changes

- dependencies-hierarchy@11.0.4

## 7.0.3

### Patch Changes

- dependencies-hierarchy@11.0.3

## 7.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - dependencies-hierarchy@11.0.2
  - @pnpm/read-package-json@6.0.2
  - @pnpm/read-project-manifest@3.0.2

## 7.0.1

### Patch Changes

- dependencies-hierarchy@11.0.1
- @pnpm/read-package-json@6.0.1
- @pnpm/read-project-manifest@3.0.1

## 7.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - dependencies-hierarchy@11.0.0
  - @pnpm/matcher@3.0.0
  - @pnpm/read-package-json@6.0.0
  - @pnpm/read-project-manifest@3.0.0

## 6.3.3

### Patch Changes

- @pnpm/read-package-json@5.0.12
- @pnpm/read-project-manifest@2.0.13
- dependencies-hierarchy@10.0.25

## 6.3.2

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - dependencies-hierarchy@10.0.24
  - @pnpm/read-package-json@5.0.11
  - @pnpm/read-project-manifest@2.0.12

## 6.3.1

### Patch Changes

- dependencies-hierarchy@10.0.23

## 6.3.0

### Minor Changes

- 57af1b1b5: pnpm list to show information whether the package is private or not

## 6.2.19

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - dependencies-hierarchy@10.0.22
  - @pnpm/read-package-json@5.0.10
  - @pnpm/read-project-manifest@2.0.11

## 6.2.18

### Patch Changes

- dependencies-hierarchy@10.0.21

## 6.2.17

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - dependencies-hierarchy@10.0.20
  - @pnpm/read-package-json@5.0.9
  - @pnpm/read-project-manifest@2.0.10

## 6.2.16

### Patch Changes

- dependencies-hierarchy@10.0.19

## 6.2.15

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - dependencies-hierarchy@10.0.18
  - @pnpm/read-package-json@5.0.8
  - @pnpm/read-project-manifest@2.0.9

## 6.2.14

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - dependencies-hierarchy@10.0.17
  - @pnpm/read-package-json@5.0.7
  - @pnpm/read-project-manifest@2.0.8

## 6.2.13

### Patch Changes

- dependencies-hierarchy@10.0.16

## 6.2.12

### Patch Changes

- dependencies-hierarchy@10.0.15

## 6.2.11

### Patch Changes

- dependencies-hierarchy@10.0.14

## 6.2.10

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - dependencies-hierarchy@10.0.13
  - @pnpm/read-package-json@5.0.6
  - @pnpm/read-project-manifest@2.0.7

## 6.2.9

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - dependencies-hierarchy@10.0.12
  - @pnpm/read-package-json@5.0.5
  - @pnpm/read-project-manifest@2.0.6

## 6.2.8

### Patch Changes

- c024e7fae: Update cli-columns to v4.

## 6.2.7

### Patch Changes

- dependencies-hierarchy@10.0.11

## 6.2.6

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - dependencies-hierarchy@10.0.10
  - @pnpm/read-package-json@5.0.4
  - @pnpm/read-project-manifest@2.0.5

## 6.2.5

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - dependencies-hierarchy@10.0.9
  - @pnpm/read-package-json@5.0.3
  - @pnpm/read-project-manifest@2.0.4

## 6.2.4

### Patch Changes

- dependencies-hierarchy@10.0.8

## 6.2.3

### Patch Changes

- dependencies-hierarchy@10.0.7

## 6.2.2

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - dependencies-hierarchy@10.0.6
  - @pnpm/read-package-json@5.0.2
  - @pnpm/read-project-manifest@2.0.3

## 6.2.1

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
  - dependencies-hierarchy@10.0.5

## 6.2.0

### Minor Changes

- 6f3fa2233: Feat - add path as part of list command json output

### Patch Changes

- Updated dependencies [a7de89feb]
  - dependencies-hierarchy@10.0.4

## 6.1.3

### Patch Changes

- dependencies-hierarchy@10.0.3

## 6.1.2

### Patch Changes

- @pnpm/read-project-manifest@2.0.2

## 6.1.1

### Patch Changes

- Updated dependencies [6e9c112af]
- Updated dependencies [97c64bae4]
  - @pnpm/read-project-manifest@2.0.1
  - @pnpm/types@7.1.0
  - dependencies-hierarchy@10.0.2
  - @pnpm/read-package-json@5.0.1

## 6.1.0

### Minor Changes

- 1729f7b99: New option added: `showExtraneous`. When `showExtraneous` is `false`, unsaved dependencies are not listed.

## 6.0.1

### Patch Changes

- dependencies-hierarchy@10.0.1

## 6.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
  - dependencies-hierarchy@10.0.0
  - @pnpm/matcher@2.0.0
  - @pnpm/read-package-json@5.0.0
  - @pnpm/read-project-manifest@2.0.0
  - @pnpm/types@7.0.0

## 5.0.26

### Patch Changes

- Updated dependencies [d853fb14a]
  - @pnpm/read-package-json@4.0.0
  - dependencies-hierarchy@9.0.19

## 5.0.25

### Patch Changes

- dependencies-hierarchy@9.0.18

## 5.0.24

### Patch Changes

- Updated dependencies [ad113645b]
  - @pnpm/read-project-manifest@1.1.7

## 5.0.23

### Patch Changes

- c70f36678: Remove redundant empty lines when run `pnpm why --parseable`

## 5.0.22

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - dependencies-hierarchy@9.0.17
  - @pnpm/read-package-json@3.1.9
  - @pnpm/read-project-manifest@1.1.6

## 5.0.21

### Patch Changes

- dependencies-hierarchy@9.0.16

## 5.0.20

### Patch Changes

- f1dc3c872: format package name in ls command

## 5.0.19

### Patch Changes

- dependencies-hierarchy@9.0.15

## 5.0.18

### Patch Changes

- dependencies-hierarchy@9.0.14

## 5.0.17

### Patch Changes

- dependencies-hierarchy@9.0.13

## 5.0.16

### Patch Changes

- dependencies-hierarchy@9.0.12

## 5.0.15

### Patch Changes

- @pnpm/read-package-json@3.1.8
- @pnpm/read-project-manifest@1.1.5
- dependencies-hierarchy@9.0.11

## 5.0.14

### Patch Changes

- dependencies-hierarchy@9.0.10

## 5.0.13

### Patch Changes

- dependencies-hierarchy@9.0.9
- @pnpm/read-project-manifest@1.1.4

## 5.0.12

### Patch Changes

- dependencies-hierarchy@9.0.8
- @pnpm/read-project-manifest@1.1.3

## 5.0.11

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - dependencies-hierarchy@9.0.7
  - @pnpm/read-package-json@3.1.7
  - @pnpm/read-project-manifest@1.1.2

## 5.0.10

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [212671848]
  - @pnpm/types@6.3.0
  - @pnpm/read-package-json@3.1.6
  - dependencies-hierarchy@9.0.6
  - @pnpm/read-project-manifest@1.1.1

## 5.0.9

### Patch Changes

- Updated dependencies [2762781cc]
  - @pnpm/read-project-manifest@1.1.0

## 5.0.8

### Patch Changes

- @pnpm/read-package-json@3.1.5
- @pnpm/read-project-manifest@1.0.13
- dependencies-hierarchy@9.0.5

## 5.0.7

### Patch Changes

- Updated dependencies [9f5803187]
  - @pnpm/read-package-json@3.1.4
  - dependencies-hierarchy@9.0.4

## 5.0.6

### Patch Changes

- @pnpm/read-project-manifest@1.0.12
- dependencies-hierarchy@9.0.3

## 5.0.5

### Patch Changes

- aa21a2df3: Print the legend only once.

## 5.0.4

### Patch Changes

- @pnpm/read-project-manifest@1.0.11

## 5.0.3

### Patch Changes

- Updated dependencies [3bd3253e3]
  - @pnpm/read-project-manifest@1.0.10
  - dependencies-hierarchy@9.0.2

## 5.0.2

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - dependencies-hierarchy@9.0.1

## 5.0.1

### Patch Changes

- 78c2741b6: Don't use inverse as it doesn't look good in all consoles.

## 5.0.0

### Major Changes

- c776db1a7: Look for dependencies at the correct location.
  The dependency paths have changed in the `node_modules` created by pnpm v5.

### Patch Changes

- 674376757: Highlight searched items with inverse. Works better in terminals with light theme.
- 220896511: Remove common-tags from dependencies.
- Updated dependencies [c776db1a7]
- Updated dependencies [db17f6f7b]
  - dependencies-hierarchy@9.0.0
  - @pnpm/types@6.2.0
  - @pnpm/read-package-json@3.1.3
  - @pnpm/read-project-manifest@1.0.9

## 4.0.30

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/matcher@1.0.3
  - dependencies-hierarchy@8.0.23
  - @pnpm/read-package-json@3.1.2
  - @pnpm/read-project-manifest@1.0.8

## 4.0.29

### Patch Changes

- e934b1a48: Update chalk to v4.1.0.
- Updated dependencies [57c510f00]
  - @pnpm/read-project-manifest@1.0.7
  - dependencies-hierarchy@8.0.22

## 4.0.28

### Patch Changes

- d3ddd023c: Update p-limit to v3.

## 4.0.27

### Patch Changes

- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0
  - dependencies-hierarchy@8.0.21
  - @pnpm/matcher@1.0.3
  - @pnpm/read-package-json@3.1.1
  - @pnpm/read-project-manifest@1.0.6

## 4.0.27-alpha.2

### Patch Changes

- dependencies-hierarchy@8.0.21-alpha.2

## 4.0.27-alpha.1

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - dependencies-hierarchy@8.0.21-alpha.1
  - @pnpm/read-package-json@3.1.1-alpha.0
  - @pnpm/read-project-manifest@1.0.6-alpha.0

## 4.0.27-alpha.0

### Patch Changes

- dependencies-hierarchy@8.0.21-alpha.0

## 4.0.26

### Patch Changes

- Updated dependencies [907c63a48]
  - @pnpm/matcher@1.0.2
  - dependencies-hierarchy@8.0.20
  - @pnpm/read-project-manifest@1.0.5
