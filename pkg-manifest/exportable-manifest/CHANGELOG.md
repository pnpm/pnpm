# @pnpm/exportable-manifest

## 6.0.2

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/read-project-manifest@6.0.2

## 6.0.1

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/read-project-manifest@6.0.1

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- 3477ee5: pnpm will now check the `package.json` file for a `packageManager` field. If this field is present and specifies a different package manager or a different version of pnpm than the one you're currently using, pnpm will not proceed. This ensures that you're always using the correct package manager and version that the project requires.

  To disable this behaviour, set the `package-manager-strict` setting to `false` or the `COREPACK_ENABLE_STRICT` env variable to `0`.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/error@6.0.0
  - @pnpm/read-project-manifest@6.0.0

## 5.0.11

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2
  - @pnpm/read-project-manifest@5.0.10

## 5.0.10

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/read-project-manifest@5.0.9

## 5.0.9

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/read-project-manifest@5.0.8

## 5.0.8

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/read-project-manifest@5.0.7

## 5.0.7

### Patch Changes

- @pnpm/read-project-manifest@5.0.6

## 5.0.6

### Patch Changes

- e9aa6f682: Apply fixes from @typescript-eslint v6 for nullish coalescing and optional chains. No behavior changes are expected with this change.
  - @pnpm/read-project-manifest@5.0.5

## 5.0.5

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/read-project-manifest@5.0.4

## 5.0.4

### Patch Changes

- Updated dependencies [b4892acc5]
  - @pnpm/read-project-manifest@5.0.3

## 5.0.3

### Patch Changes

- @pnpm/error@5.0.2
- @pnpm/read-project-manifest@5.0.2

## 5.0.2

### Patch Changes

- 4b97f1f07: Don't use await in loops.

## 5.0.1

### Patch Changes

- Updated dependencies [a9e0b7cbf]
  - @pnpm/types@9.1.0
  - @pnpm/read-project-manifest@5.0.1
  - @pnpm/error@5.0.1

## 5.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/read-project-manifest@5.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0

## 4.0.8

### Patch Changes

- @pnpm/read-project-manifest@4.1.4

## 4.0.7

### Patch Changes

- b71c6ed74: Fix version number replacing for namespaced workspace packages. `workspace:@foo/bar@*` should be replaced with `npm:@foo/bar@<version>` on publish [#6052](https://github.com/pnpm/pnpm/pull/6052).

## 4.0.6

### Patch Changes

- @pnpm/error@4.0.1
- @pnpm/read-project-manifest@4.1.3

## 4.0.5

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/read-project-manifest@4.1.2

## 4.0.4

### Patch Changes

- @pnpm/read-project-manifest@4.1.1

## 4.0.3

### Patch Changes

- Updated dependencies [fec9e3149]
- Updated dependencies [0d12d38fd]
  - @pnpm/read-project-manifest@4.1.0

## 4.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - @pnpm/read-project-manifest@4.0.2

## 4.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/read-project-manifest@4.0.1

## 4.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/read-project-manifest@4.0.0

## 3.1.6

### Patch Changes

- @pnpm/read-project-manifest@3.0.13

## 3.1.5

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/read-project-manifest@3.0.12

## 3.1.4

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/read-project-manifest@3.0.11

## 3.1.3

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/read-project-manifest@3.0.10

## 3.1.2

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [39c040127]
  - @pnpm/read-project-manifest@3.0.9

## 3.1.1

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/read-project-manifest@3.0.8

## 3.1.0

### Minor Changes

- eb2426cf8: Accept the module directory path where the dependency's manifest should be read.

### Patch Changes

- Updated dependencies [01c5834bf]
  - @pnpm/read-project-manifest@3.0.7

## 3.0.7

### Patch Changes

- 5f643f23b: Update ramda to v0.28.

## 3.0.6

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/read-project-manifest@3.0.6

## 3.0.5

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/read-project-manifest@3.0.5

## 3.0.4

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0
  - @pnpm/read-project-manifest@3.0.4

## 3.0.3

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/read-project-manifest@3.0.3

## 3.0.2

### Patch Changes

- 18ba5e2c0: Add typesVersions to PUBLISH_CONFIG_WHITELIST
- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/read-project-manifest@3.0.2

## 3.0.1

### Patch Changes

- @pnpm/error@3.0.1
- @pnpm/read-project-manifest@3.0.1

## 3.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/error@3.0.0
  - @pnpm/read-project-manifest@3.0.0

## 2.3.2

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/read-project-manifest@2.0.13

## 2.3.1

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/read-project-manifest@2.0.12

## 2.3.0

### Minor Changes

- e1b459008: Remove meaningless keys from `publishConfig` when the `pack` or `publish` commands are used [#4311](https://github.com/pnpm/pnpm/issues/4311)

## 2.2.4

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/read-project-manifest@2.0.11

## 2.2.3

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/read-project-manifest@2.0.10

## 2.2.2

### Patch Changes

- 6493e0c93: add readme file to published package.json file
- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/read-project-manifest@2.0.9

## 2.2.1

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/read-project-manifest@2.0.8

## 2.2.0

### Minor Changes

- 6428690e2: Allow to set `os` and `cpu` in `publishConfig`.

## 2.1.8

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/read-project-manifest@2.0.7

## 2.1.7

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/read-project-manifest@2.0.6

## 2.1.6

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/read-project-manifest@2.0.5

## 2.1.5

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/read-project-manifest@2.0.4

## 2.1.4

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/read-project-manifest@2.0.3

## 2.1.3

### Patch Changes

- a1a03d145: Import only the required functions from ramda.

## 2.1.2

### Patch Changes

- 6a1468495: Adds support for `type` and `imports` in publishConfig

## 2.1.1

### Patch Changes

- @pnpm/read-project-manifest@2.0.2

## 2.1.0

### Minor Changes

- 85fb21a83: Add support for workspace:^ and workspace:~ aliases

### Patch Changes

- Updated dependencies [6e9c112af]
- Updated dependencies [97c64bae4]
  - @pnpm/read-project-manifest@2.0.1
  - @pnpm/types@7.1.0

## 2.0.1

### Patch Changes

- 561276d2c: Remove publish lifecycle events from manifest to avoid npm running them.

## 2.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
  - @pnpm/error@2.0.0
  - @pnpm/read-project-manifest@2.0.0
  - @pnpm/types@7.0.0

## 1.2.2

### Patch Changes

- Updated dependencies [ad113645b]
  - @pnpm/read-project-manifest@1.1.7

## 1.2.1

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/read-project-manifest@1.1.6

## 1.2.0

### Minor Changes

- c854f8547: Remove the "pnpm" property that stores pnpm settings from the manifest.

## 1.1.0

### Minor Changes

- 284e95c5e: Convert relative workspace paths to version specs.
- 084614f55: Support aliases to workspace packages. For instance, `"foo": "workspace:bar@*"` will link bar from the repository but aliased to foo. Before publish, these specs are converted to regular aliased versions.

## 1.0.8

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/read-project-manifest@1.1.5

## 1.0.7

### Patch Changes

- @pnpm/read-project-manifest@1.1.4

## 1.0.6

### Patch Changes

- @pnpm/read-project-manifest@1.1.3

## 1.0.5

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/read-project-manifest@1.1.2

## 1.0.4

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/read-project-manifest@1.1.1

## 1.0.3

### Patch Changes

- Updated dependencies [2762781cc]
  - @pnpm/read-project-manifest@1.1.0

## 1.0.2

### Patch Changes

- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/read-project-manifest@1.0.13

## 1.0.1

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/read-project-manifest@1.0.12

## 1.0.0

### Major Changes

- edf1f412e: Package created.
