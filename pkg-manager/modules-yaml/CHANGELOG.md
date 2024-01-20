# @pnpm/modules-yaml

## 12.1.7

### Patch Changes

- d349bc3a2: readModulesYaml should not crash on empty file.

## 12.1.6

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2

## 12.1.5

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1

## 12.1.4

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0

## 12.1.3

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0

## 12.1.2

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0

## 12.1.1

### Patch Changes

- Updated dependencies [a9e0b7cbf]
  - @pnpm/types@9.1.0

## 12.1.0

### Minor Changes

- e6b83c84e: Do not create a `node_modules` folder with a `.modules.yaml` file if there are now dependencies inside `node_modules`.

## 12.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/types@9.0.0

## 11.1.0

### Minor Changes

- 2458741fa: New field saved in the modules state file: `hoistedLocations`. This field maps the locations of dependencies, when `node-linker=hoisted` is used.

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0

## 11.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0

## 11.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0

## 11.0.0

### Major Changes

- 72f7d6b3b: Export readModulesManifest and writeModulesManifest instead of read and write.

## 10.0.8

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0

## 10.0.7

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0

## 10.0.6

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0

## 10.0.5

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0

## 10.0.4

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0

## 10.0.3

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0

## 10.0.2

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0

## 10.0.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1

## 10.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0

## 9.1.1

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0

## 9.1.0

### Minor Changes

- cdc521cfa: New field added: injectedDeps.

## 9.0.11

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0

## 9.0.10

### Patch Changes

- 7375396db: Save the value of the active `nodeLinker` to `node_modules/.modules.yaml`.

## 9.0.9

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0

## 9.0.8

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1

## 9.0.7

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0

## 9.0.6

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0

## 9.0.5

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0

## 9.0.4

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0

## 9.0.3

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0

## 9.0.2

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0

## 9.0.1

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0

## 9.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.
- 78470a32d: New required option added to the modules meta object: `prunedAt`. `prunedAt` is the stringified UTC date when the virtual store was last cleared.

### Patch Changes

- Updated dependencies [97b986fbc]
  - @pnpm/types@7.0.0

## 8.0.6

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0

## 8.0.5

### Patch Changes

- 09492b7b4: Update write-file-atomic to v3.

## 8.0.4

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1

## 8.0.3

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0

## 8.0.2

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 8.0.1

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0

## 8.0.0

### Major Changes

- 71a8c8ce3: Breaking changes to the `node_modules/.modules.yaml` file:
  - `hoistedAliases` replaced with `hoistedDependencies`.
  - `shamefullyHoist` replaced with `publicHoistPattern`.

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0

## 7.0.0

### Major Changes

- 3f73eaf0c: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- 802d145fc: Remove `independent-leaves` support.

### Patch Changes

- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0

## 7.0.0-alpha.0

### Major Changes

- 3f73eaf0: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0

## 6.0.2

### Patch Changes

- 907c63a48: Dependencies updated.
