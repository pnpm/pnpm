# @pnpm/link-bins

## 9.0.12

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2
  - @pnpm/package-bins@8.0.6
  - @pnpm/manifest-utils@5.0.7
  - @pnpm/read-package-json@8.0.7
  - @pnpm/read-project-manifest@5.0.10

## 9.0.11

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/package-bins@8.0.5
  - @pnpm/manifest-utils@5.0.6
  - @pnpm/read-package-json@8.0.6
  - @pnpm/read-project-manifest@5.0.9

## 9.0.10

### Patch Changes

- Updated dependencies [43ce9e4a6]
- Updated dependencies [d2dc2e66a]
  - @pnpm/types@9.4.0
  - @pnpm/package-bins@8.0.4
  - @pnpm/manifest-utils@5.0.5
  - @pnpm/read-package-json@8.0.5
  - @pnpm/read-project-manifest@5.0.8

## 9.0.9

### Patch Changes

- 5c8c9196c: Improved the performance of linking bins of hoisted dependencies to `node_modules/.pnpm/node_modules/.bin` [#7212](https://github.com/pnpm/pnpm/pull/7212).

## 9.0.8

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/package-bins@8.0.3
  - @pnpm/manifest-utils@5.0.4
  - @pnpm/read-package-json@8.0.4
  - @pnpm/read-project-manifest@5.0.7

## 9.0.7

### Patch Changes

- @pnpm/read-project-manifest@5.0.6

## 9.0.6

### Patch Changes

- @pnpm/read-project-manifest@5.0.5

## 9.0.5

### Patch Changes

- e26d15c6d: When a command cannot be created in `.bin`, print the exact error message that happened.
- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/package-bins@8.0.2
  - @pnpm/manifest-utils@5.0.3
  - @pnpm/read-package-json@8.0.3
  - @pnpm/read-project-manifest@5.0.4

## 9.0.4

### Patch Changes

- Updated dependencies [b4892acc5]
  - @pnpm/read-project-manifest@5.0.3

## 9.0.3

### Patch Changes

- @pnpm/error@5.0.2
- @pnpm/manifest-utils@5.0.2
- @pnpm/read-package-json@8.0.2
- @pnpm/read-project-manifest@5.0.2

## 9.0.2

### Patch Changes

- Updated dependencies [4b97f1f07]
  - @pnpm/read-modules-dir@6.0.1

## 9.0.1

### Patch Changes

- Updated dependencies [a9e0b7cbf]
- Updated dependencies [a9e0b7cbf]
  - @pnpm/types@9.1.0
  - @pnpm/manifest-utils@5.0.1
  - @pnpm/package-bins@8.0.1
  - @pnpm/read-package-json@8.0.1
  - @pnpm/read-project-manifest@5.0.1
  - @pnpm/error@5.0.1

## 9.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/read-project-manifest@5.0.0
  - @pnpm/read-package-json@8.0.0
  - @pnpm/manifest-utils@5.0.0
  - @pnpm/package-bins@8.0.0
  - @pnpm/read-modules-dir@6.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0

## 8.0.11

### Patch Changes

- 685b3a7ea: New directories should be prepended to NODE_PATH in command shims, not appended.

## 8.0.10

### Patch Changes

- f9c30c6d7: Command shim should not set higher priority to the `node_modules/.pnpm/node_modules` directory through the `NODE_PATH` env variable, then the command's own `node_modules` directory [#5176](https://github.com/pnpm/pnpm/issues/5176).

## 8.0.9

### Patch Changes

- @pnpm/read-project-manifest@4.1.4

## 8.0.8

### Patch Changes

- 90d26c449: Don't crash when a bin file is not found and `prefer-symlinked-executables` is `true` [#5946](https://github.com/pnpm/pnpm/pull/5946).

## 8.0.7

### Patch Changes

- 4008a5236: Ensure the permission of bin file when `prefer-symlinked-executables` is set to `true` [#5913](https://github.com/pnpm/pnpm/pull/5913).

## 8.0.6

### Patch Changes

- @pnpm/error@4.0.1
- @pnpm/manifest-utils@4.1.4
- @pnpm/read-package-json@7.0.5
- @pnpm/read-project-manifest@4.1.3

## 8.0.5

### Patch Changes

- bc18d33fe: Allow the `-S` flag in command shims [pnpm/cmd-shim#42](https://github.com/pnpm/cmd-shim/pull/42).
- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/package-bins@7.0.3
  - @pnpm/manifest-utils@4.1.3
  - @pnpm/read-package-json@7.0.4
  - @pnpm/read-project-manifest@4.1.2

## 8.0.4

### Patch Changes

- a9d59d8bc: Update dependencies.
- Updated dependencies [c245edf1b]
- Updated dependencies [a9d59d8bc]
  - @pnpm/manifest-utils@4.1.2
  - @pnpm/read-package-json@7.0.3
  - @pnpm/read-project-manifest@4.1.1

## 8.0.3

### Patch Changes

- Updated dependencies [fec9e3149]
- Updated dependencies [0d12d38fd]
  - @pnpm/read-project-manifest@4.1.0

## 8.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - @pnpm/manifest-utils@4.1.1
  - @pnpm/package-bins@7.0.2
  - @pnpm/read-package-json@7.0.2
  - @pnpm/read-project-manifest@4.0.2

## 8.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/manifest-utils@4.1.0
  - @pnpm/package-bins@7.0.1
  - @pnpm/read-package-json@7.0.1
  - @pnpm/read-project-manifest@4.0.1

## 8.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/manifest-utils@4.0.0
  - @pnpm/package-bins@7.0.0
  - @pnpm/read-modules-dir@5.0.0
  - @pnpm/read-package-json@7.0.0
  - @pnpm/read-project-manifest@4.0.0

## 7.2.10

### Patch Changes

- @pnpm/read-project-manifest@3.0.13

## 7.2.9

### Patch Changes

- @pnpm/manifest-utils@3.1.6

## 7.2.8

### Patch Changes

- 5eb41a551: When linking commands to a directory, remove any .exe files that are already present in that target directory by the same name.

  This fixes an issue with pnpm global update on Windows. If pnpm was installed with the standalone script and then updated with pnpm using `pnpm add --global pnpm`, the exe file initially created by the standalone script should be removed.

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/manifest-utils@3.1.5
  - @pnpm/read-package-json@6.0.11
  - @pnpm/read-project-manifest@3.0.12

## 7.2.7

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/manifest-utils@3.1.4
  - @pnpm/package-bins@6.0.8
  - @pnpm/read-package-json@6.0.10
  - @pnpm/read-project-manifest@3.0.11

## 7.2.6

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/manifest-utils@3.1.3
  - @pnpm/package-bins@6.0.7
  - @pnpm/read-package-json@6.0.9
  - @pnpm/read-project-manifest@3.0.10

## 7.2.5

### Patch Changes

- 07bc24ad1: Update bin-links.
- Updated dependencies [07bc24ad1]
  - @pnpm/read-package-json@6.0.8

## 7.2.4

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [39c040127]
  - @pnpm/read-project-manifest@3.0.9

## 7.2.3

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/manifest-utils@3.1.2
  - @pnpm/package-bins@6.0.6
  - @pnpm/read-package-json@6.0.7
  - @pnpm/read-project-manifest@3.0.8

## 7.2.2

### Patch Changes

- Updated dependencies [01c5834bf]
  - @pnpm/read-project-manifest@3.0.7

## 7.2.1

### Patch Changes

- Updated dependencies [e3f4d131c]
  - @pnpm/manifest-utils@3.1.1

## 7.2.0

### Minor Changes

- 28f000509: A new setting supported: `prefer-symlinked-executables`. When `true`, pnpm will create symlinks to executables in
  `node_modules/.bin` instead of command shims (but on POSIX systems only).

  This setting is `true` by default when `node-linker` is set to `hoisted`.

  Related issue: [#4782](https://github.com/pnpm/pnpm/issues/4782).

## 7.1.7

### Patch Changes

- Updated dependencies [f5621a42c]
  - @pnpm/manifest-utils@3.1.0

## 7.1.6

### Patch Changes

- 5f643f23b: Update ramda to v0.28.

## 7.1.5

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/manifest-utils@3.0.6
  - @pnpm/package-bins@6.0.5
  - @pnpm/read-package-json@6.0.6
  - @pnpm/read-project-manifest@3.0.6

## 7.1.4

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/manifest-utils@3.0.5
  - @pnpm/package-bins@6.0.4
  - @pnpm/read-package-json@6.0.5
  - @pnpm/read-project-manifest@3.0.5

## 7.1.3

### Patch Changes

- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0
  - @pnpm/manifest-utils@3.0.4
  - @pnpm/package-bins@6.0.3
  - @pnpm/read-package-json@6.0.4
  - @pnpm/read-project-manifest@3.0.4

## 7.1.2

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/manifest-utils@3.0.3
  - @pnpm/package-bins@6.0.2
  - @pnpm/read-package-json@6.0.3
  - @pnpm/read-project-manifest@3.0.3

## 7.1.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/manifest-utils@3.0.2
  - @pnpm/package-bins@6.0.1
  - @pnpm/read-package-json@6.0.2
  - @pnpm/read-project-manifest@3.0.2

## 7.1.0

### Minor Changes

- 8fa95fd86: New option added: `extraNodePaths`.

### Patch Changes

- Updated dependencies [618842b0d]
  - @pnpm/manifest-utils@3.0.1
  - @pnpm/error@3.0.1
  - @pnpm/read-package-json@6.0.1
  - @pnpm/read-project-manifest@3.0.1

## 7.0.0

### Major Changes

- 516859178: `extendNodePath` removed.
- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/error@3.0.0
  - @pnpm/manifest-utils@3.0.0
  - @pnpm/package-bins@6.0.0
  - @pnpm/read-modules-dir@4.0.0
  - @pnpm/read-package-json@6.0.0
  - @pnpm/read-project-manifest@3.0.0

## 6.2.12

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/manifest-utils@2.1.9
  - @pnpm/read-package-json@5.0.12
  - @pnpm/read-project-manifest@2.0.13

## 6.2.11

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/manifest-utils@2.1.8
  - @pnpm/package-bins@5.0.12
  - @pnpm/read-package-json@5.0.11
  - @pnpm/read-project-manifest@2.0.12

## 6.2.10

### Patch Changes

- Updated dependencies [8a2cad034]
  - @pnpm/manifest-utils@2.1.7

## 6.2.9

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/manifest-utils@2.1.6
  - @pnpm/package-bins@5.0.11
  - @pnpm/read-package-json@5.0.10
  - @pnpm/read-project-manifest@2.0.11

## 6.2.8

### Patch Changes

- 701ea0746: Don't throw an error during install when the bin of a dependency points to a path that doesn't exist [#3763](https://github.com/pnpm/pnpm/issues/3763).
- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/manifest-utils@2.1.5
  - @pnpm/package-bins@5.0.10
  - @pnpm/read-package-json@5.0.9
  - @pnpm/read-project-manifest@2.0.10

## 6.2.7

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/manifest-utils@2.1.4
  - @pnpm/package-bins@5.0.9
  - @pnpm/read-package-json@5.0.8
  - @pnpm/read-project-manifest@2.0.9

## 6.2.6

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/manifest-utils@2.1.3
  - @pnpm/package-bins@5.0.8
  - @pnpm/read-package-json@5.0.7
  - @pnpm/read-project-manifest@2.0.8

## 6.2.5

### Patch Changes

- bb0f8bc16: Don't crash if a bin file cannot be created because the source files could not be found.

## 6.2.4

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/manifest-utils@2.1.2
  - @pnpm/package-bins@5.0.7
  - @pnpm/read-package-json@5.0.6
  - @pnpm/read-project-manifest@2.0.7

## 6.2.3

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/manifest-utils@2.1.1
  - @pnpm/package-bins@5.0.6
  - @pnpm/read-package-json@5.0.5
  - @pnpm/read-project-manifest@2.0.6

## 6.2.2

### Patch Changes

- a916accec: Do not warn about bin conflicts, just log a debug message.

## 6.2.1

### Patch Changes

- 6375cdce0: Autofix command files with Windows line endings on the shebang line.

## 6.2.0

### Minor Changes

- c7081cbb4: New option added: `extendNodePath`. When it is set to `false`, pnpm does not set the `NODE_PATH` environment variable in the command shims.

### Patch Changes

- 0d4a7c69e: Pick the right extension for command files. It is important to write files with .CMD extension on case sensitive Windows drives.

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
