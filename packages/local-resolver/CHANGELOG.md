# @pnpm/local-resolver

## 8.0.13

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/read-project-manifest@3.0.11
  - @pnpm/resolver-base@9.1.2

## 8.0.12

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/read-project-manifest@3.0.10
  - @pnpm/resolver-base@9.1.1

## 8.0.11

### Patch Changes

- dbac0ca01: Update ssri to v9.

## 8.0.10

### Patch Changes

- Updated dependencies [23984abd1]
  - @pnpm/resolver-base@9.1.0

## 8.0.9

### Patch Changes

- Updated dependencies [39c040127]
  - @pnpm/read-project-manifest@3.0.9

## 8.0.8

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/read-project-manifest@3.0.8
  - @pnpm/resolver-base@9.0.6

## 8.0.7

### Patch Changes

- Updated dependencies [01c5834bf]
  - @pnpm/read-project-manifest@3.0.7

## 8.0.6

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/read-project-manifest@3.0.6
  - @pnpm/resolver-base@9.0.5

## 8.0.5

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/read-project-manifest@3.0.5
  - @pnpm/resolver-base@9.0.4

## 8.0.4

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0
  - @pnpm/read-project-manifest@3.0.4
  - @pnpm/resolver-base@9.0.3

## 8.0.3

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/read-project-manifest@3.0.3
  - @pnpm/resolver-base@9.0.2

## 8.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/read-project-manifest@3.0.2
  - @pnpm/resolver-base@9.0.1

## 8.0.1

### Patch Changes

- @pnpm/error@3.0.1
- @pnpm/read-project-manifest@3.0.1

## 8.0.0

### Major Changes

- 9c22c063e: Local dependencies referenced through the `file:` protocol are hard linked (not symlinked) [#4408](https://github.com/pnpm/pnpm/pull/4408). If you need to symlink a dependency, use the `link:` protocol instead.
- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/error@3.0.0
  - @pnpm/graceful-fs@2.0.0
  - @pnpm/read-project-manifest@3.0.0
  - @pnpm/resolver-base@9.0.0

## 7.0.8

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/read-project-manifest@2.0.13

## 7.0.7

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/read-project-manifest@2.0.12
  - @pnpm/resolver-base@8.1.6

## 7.0.6

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/read-project-manifest@2.0.11
  - @pnpm/resolver-base@8.1.5

## 7.0.5

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/read-project-manifest@2.0.10
  - @pnpm/resolver-base@8.1.4

## 7.0.4

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/read-project-manifest@2.0.9
  - @pnpm/resolver-base@8.1.3

## 7.0.3

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/read-project-manifest@2.0.8
  - @pnpm/resolver-base@8.1.2

## 7.0.2

### Patch Changes

- 631877ebf: Don't fail if a local linked directory is not found (unless it should be injected). This is the intended behavior of the "link:" protocol as per Yarn's docs.

## 7.0.1

### Patch Changes

- 108bd4a39: Injected directory resolutions should contain the relative path to the directory.
- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/read-project-manifest@2.0.7
  - @pnpm/resolver-base@8.1.1

## 7.0.0

### Major Changes

- 4ab87844a: Local directory dependencies are resolved to absolute path.

### Minor Changes

- 4ab87844a: Support the resolution of injected local dependencies.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/resolver-base@8.1.0
  - @pnpm/read-project-manifest@2.0.6

## 6.1.0

### Minor Changes

- 3f0178b4c: Allow to link a directory that has no manifest file.

## 6.0.5

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/read-project-manifest@2.0.5
  - @pnpm/resolver-base@8.0.4

## 6.0.4

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/read-project-manifest@2.0.4
  - @pnpm/resolver-base@8.0.3

## 6.0.3

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/read-project-manifest@2.0.3
  - @pnpm/resolver-base@8.0.2

## 6.0.2

### Patch Changes

- Updated dependencies [a2aeeef88]
  - @pnpm/graceful-fs@1.0.0
  - @pnpm/read-project-manifest@2.0.2

## 6.0.1

### Patch Changes

- Updated dependencies [6e9c112af]
- Updated dependencies [97c64bae4]
  - @pnpm/read-project-manifest@2.0.1
  - @pnpm/types@7.1.0
  - @pnpm/resolver-base@8.0.1

## 6.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 83645c8ed: Update ssri.
- Updated dependencies [97b986fbc]
  - @pnpm/error@2.0.0
  - @pnpm/read-project-manifest@2.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/types@7.0.0

## 5.1.3

### Patch Changes

- ad113645b: pin graceful-fs to v4.2.4
- Updated dependencies [ad113645b]
  - @pnpm/read-project-manifest@1.1.7

## 5.1.2

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/read-project-manifest@1.1.6
  - @pnpm/resolver-base@7.1.1

## 5.1.1

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/resolver-base@7.1.0

## 5.1.0

### Minor Changes

- 284e95c5e: Support relative path to workspace directory.

## 5.0.20

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/read-project-manifest@1.1.5

## 5.0.19

### Patch Changes

- @pnpm/read-project-manifest@1.1.4

## 5.0.18

### Patch Changes

- @pnpm/read-project-manifest@1.1.3

## 5.0.17

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/read-project-manifest@1.1.2
  - @pnpm/resolver-base@7.0.5

## 5.0.16

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/read-project-manifest@1.1.1
  - @pnpm/resolver-base@7.0.4

## 5.0.15

### Patch Changes

- Updated dependencies [2762781cc]
  - @pnpm/read-project-manifest@1.1.0

## 5.0.14

### Patch Changes

- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/read-project-manifest@1.0.13

## 5.0.13

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/read-project-manifest@1.0.12

## 5.0.12

### Patch Changes

- @pnpm/read-project-manifest@1.0.11

## 5.0.11

### Patch Changes

- Updated dependencies [3bd3253e3]
  - @pnpm/read-project-manifest@1.0.10

## 5.0.10

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/read-project-manifest@1.0.9
  - @pnpm/resolver-base@7.0.3

## 5.0.9

### Patch Changes

- 1520e3d6f: Update graceful-fs to v4.2.4

## 5.0.8

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/read-project-manifest@1.0.8
  - @pnpm/resolver-base@7.0.2

## 5.0.7

### Patch Changes

- Updated dependencies [57c510f00]
  - @pnpm/read-project-manifest@1.0.7

## 5.0.6

### Patch Changes

- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0
  - @pnpm/error@1.2.1
  - @pnpm/read-project-manifest@1.0.6
  - @pnpm/resolver-base@7.0.1

## 5.0.6-alpha.0

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/read-project-manifest@1.0.6-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0

## 5.0.5

### Patch Changes

- @pnpm/read-project-manifest@1.0.5
