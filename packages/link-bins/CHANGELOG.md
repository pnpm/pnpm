# @pnpm/link-bins

## 6.1.0

### Minor Changes

- 83e23601e: `linkBins()` accepts the project manifest and prioritizes the bins of its direct dependencies over the bin files of the hoisted dependencies.
- 553a5d840: Allow to specify the path to Node.js executable that should be called from the command shim.

### Patch Changes

- Updated dependencies [553a5d840]
  - @pnpm/manifest-utils@2.1.0

## 6.0.8

### Patch Changes

- Updated dependencies [97f90e537]
  - @pnpm/package-bins@5.0.5

## 6.0.7

### Patch Changes

- Updated dependencies [71aab049d]
  - @pnpm/read-modules-dir@3.0.1

## 6.0.6

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/package-bins@5.0.4
  - @pnpm/read-package-json@5.0.4
  - @pnpm/read-project-manifest@2.0.5

## 6.0.5

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/package-bins@5.0.3
  - @pnpm/read-package-json@5.0.3
  - @pnpm/read-project-manifest@2.0.4

## 6.0.4

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/package-bins@5.0.2
  - @pnpm/read-package-json@5.0.2
  - @pnpm/read-project-manifest@2.0.3

## 6.0.3

### Patch Changes

- a1a03d145: Import only the required functions from ramda.

## 6.0.2

### Patch Changes

- @pnpm/read-project-manifest@2.0.2

## 6.0.1

### Patch Changes

- Updated dependencies [6e9c112af]
- Updated dependencies [97c64bae4]
  - @pnpm/read-project-manifest@2.0.1
  - @pnpm/types@7.1.0
  - @pnpm/package-bins@5.0.1
  - @pnpm/read-package-json@5.0.1

## 6.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 06c6c9959: Don't create a PowerShell command shim for pnpm commands.
- Updated dependencies [97b986fbc]
  - @pnpm/error@2.0.0
  - @pnpm/package-bins@5.0.0
  - @pnpm/read-modules-dir@3.0.0
  - @pnpm/read-package-json@5.0.0
  - @pnpm/read-project-manifest@2.0.0
  - @pnpm/types@7.0.0

## 5.3.25

### Patch Changes

- d853fb14a: Don't fail when linking bins of a package that uses the `directories.bin` and points to a directory that has subdirectories.
- Updated dependencies [d853fb14a]
- Updated dependencies [d853fb14a]
  - @pnpm/package-bins@4.1.0
  - @pnpm/read-package-json@4.0.0

## 5.3.24

### Patch Changes

- 6350a3381: Don't add a non-directory to the NODE_PATH declared in the command shim.

## 5.3.23

### Patch Changes

- a78e5c47f: Don't create a PowerShell command shim for pnpm commands.

## 5.3.22

### Patch Changes

- Updated dependencies [ad113645b]
  - @pnpm/package-bins@4.0.11
  - @pnpm/read-project-manifest@1.1.7

## 5.3.21

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/package-bins@4.0.10
  - @pnpm/read-package-json@3.1.9
  - @pnpm/read-project-manifest@1.1.6

## 5.3.20

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/read-package-json@3.1.8
  - @pnpm/read-project-manifest@1.1.5

## 5.3.19

### Patch Changes

- @pnpm/read-project-manifest@1.1.4

## 5.3.18

### Patch Changes

- @pnpm/read-project-manifest@1.1.3

## 5.3.17

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/package-bins@4.0.9
  - @pnpm/read-package-json@3.1.7
  - @pnpm/read-project-manifest@1.1.2

## 5.3.16

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [212671848]
  - @pnpm/types@6.3.0
  - @pnpm/read-package-json@3.1.6
  - @pnpm/package-bins@4.0.8
  - @pnpm/read-project-manifest@1.1.1

## 5.3.15

### Patch Changes

- fb863fae4: When creating command shims, add the parent node_modules directory of the `.bin` directory to the NODE_PATH.

## 5.3.14

### Patch Changes

- 51311d3ba: Always return a result.
- Updated dependencies [2762781cc]
  - @pnpm/read-project-manifest@1.1.0

## 5.3.13

### Patch Changes

- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/read-package-json@3.1.5
  - @pnpm/read-project-manifest@1.0.13

## 5.3.12

### Patch Changes

- Updated dependencies [9f5803187]
  - @pnpm/read-package-json@3.1.4

## 5.3.11

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/read-project-manifest@1.0.12

## 5.3.10

### Patch Changes

- @pnpm/read-project-manifest@1.0.11

## 5.3.9

### Patch Changes

- Updated dependencies [3bd3253e3]
- Updated dependencies [24af41f20]
  - @pnpm/read-project-manifest@1.0.10
  - @pnpm/read-modules-dir@2.0.3

## 5.3.8

### Patch Changes

- Updated dependencies [a2ef8084f]
  - @pnpm/read-modules-dir@2.0.2

## 5.3.7

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/package-bins@4.0.7
  - @pnpm/read-package-json@3.1.3
  - @pnpm/read-project-manifest@1.0.9

## 5.3.6

### Patch Changes

- Updated dependencies [1520e3d6f]
  - @pnpm/package-bins@4.0.6

## 5.3.5

### Patch Changes

- e1ca9fc13: Update @zkochan/cmd-shim to v5.
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/package-bins@4.0.5
  - @pnpm/read-package-json@3.1.2
  - @pnpm/read-project-manifest@1.0.8

## 5.3.4

### Patch Changes

- Updated dependencies [57c510f00]
  - @pnpm/read-project-manifest@1.0.7

## 5.3.3

### Patch Changes

- Updated dependencies [da091c711]
  - @pnpm/types@6.0.0
  - @pnpm/error@1.2.1
  - @pnpm/package-bins@4.0.4
  - @pnpm/read-modules-dir@2.0.2
  - @pnpm/read-package-json@3.1.1
  - @pnpm/read-project-manifest@1.0.6

## 5.3.3-alpha.0

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/package-bins@4.0.4-alpha.0
  - @pnpm/read-package-json@3.1.1-alpha.0
  - @pnpm/read-project-manifest@1.0.6-alpha.0

## 5.3.2

### Patch Changes

- 907c63a48: Dependencies updated.
- 907c63a48: Use `fs.mkdir` instead of `make-dir`.
  - @pnpm/read-project-manifest@1.0.5
