# @pnpm/build-modules

## 9.3.5

### Patch Changes

- Updated dependencies [32915f0e4]
  - @pnpm/store-controller-types@14.1.1
  - @pnpm/lifecycle@13.1.6
  - @pnpm/link-bins@7.2.4

## 9.3.4

### Patch Changes

- 39c040127: upgrade various dependencies
- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
  - @pnpm/link-bins@7.2.4
  - @pnpm/store-controller-types@14.1.0
  - @pnpm/lifecycle@13.1.5

## 9.3.3

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/core-loggers@7.0.6
  - @pnpm/lifecycle@13.1.4
  - @pnpm/link-bins@7.2.3
  - @pnpm/read-package-json@6.0.7
  - @pnpm/store-controller-types@14.0.2

## 9.3.2

### Patch Changes

- @pnpm/link-bins@7.2.2
- @pnpm/lifecycle@13.1.3

## 9.3.1

### Patch Changes

- @pnpm/link-bins@7.2.1

## 9.3.0

### Minor Changes

- 28f000509: A new setting supported: `prefer-symlinked-executables`. When `true`, pnpm will create symlinks to executables in
  `node_modules/.bin` instead of command shims (but on POSIX systems only).

  This setting is `true` by default when `node-linker` is set to `hoisted`.

  Related issue: [#4782](https://github.com/pnpm/pnpm/issues/4782).

### Patch Changes

- Updated dependencies [28f000509]
  - @pnpm/link-bins@7.2.0

## 9.2.4

### Patch Changes

- @pnpm/link-bins@7.1.7

## 9.2.3

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/link-bins@7.1.6
  - @pnpm/lifecycle@13.1.2

## 9.2.2

### Patch Changes

- 00c12fa53: Throw an error if a patch couldn't be applied.

## 9.2.1

### Patch Changes

- 8e5b77ef6: Update the dependencies when a patch file is modified.
- 285ff09ba: Patch packages even when scripts are ignored.
- Updated dependencies [285ff09ba]
- Updated dependencies [8e5b77ef6]
  - @pnpm/calc-dep-state@3.0.1
  - @pnpm/types@8.4.0
  - @pnpm/core-loggers@7.0.5
  - @pnpm/lifecycle@13.1.1
  - @pnpm/link-bins@7.1.5
  - @pnpm/read-package-json@6.0.6
  - @pnpm/store-controller-types@14.0.1

## 9.2.0

### Minor Changes

- 2a34b21ce: Support packages patching.

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [2a34b21ce]
- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/lifecycle@13.1.0
  - @pnpm/calc-dep-state@3.0.0
  - @pnpm/store-controller-types@14.0.0
  - @pnpm/core-loggers@7.0.4
  - @pnpm/link-bins@7.1.4
  - @pnpm/read-package-json@6.0.5

## 9.1.5

### Patch Changes

- 0abfe1718: `requiresBuild` may be of any value. This is just a workaround to a typing issue. `requiresBuild` will always be boolean.
- Updated dependencies [fb5bbfd7a]
  - @pnpm/types@8.2.0
  - @pnpm/core-loggers@7.0.3
  - @pnpm/lifecycle@13.0.5
  - @pnpm/link-bins@7.1.3
  - @pnpm/read-package-json@6.0.4
  - @pnpm/store-controller-types@13.0.4

## 9.1.4

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/core-loggers@7.0.2
  - @pnpm/lifecycle@13.0.4
  - @pnpm/link-bins@7.1.2
  - @pnpm/read-package-json@6.0.3
  - @pnpm/store-controller-types@13.0.3

## 9.1.3

### Patch Changes

- 6756c2b02: It should be possible to install a git-hosted package that has no `package.json` file [#4822](https://github.com/pnpm/pnpm/issues/4822).
- Updated dependencies [6756c2b02]
  - @pnpm/store-controller-types@13.0.2
  - @pnpm/lifecycle@13.0.3
  - @pnpm/link-bins@7.1.1

## 9.1.2

### Patch Changes

- 971f2c4a5: Improve the performance of the build sequence calculation step.

## 9.1.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/core-loggers@7.0.1
  - @pnpm/lifecycle@13.0.2
  - @pnpm/link-bins@7.1.1
  - @pnpm/read-package-json@6.0.2
  - @pnpm/store-controller-types@13.0.1

## 9.1.0

### Minor Changes

- 8fa95fd86: New option added: `extraNodePaths`.

### Patch Changes

- 2109f2e8e: Use `@pnpm/graph-sequencer` instead of `graph-sequencer`.
- Updated dependencies [8fa95fd86]
  - @pnpm/link-bins@7.1.0
  - @pnpm/lifecycle@13.0.1
  - @pnpm/calc-dep-state@2.0.1
  - @pnpm/read-package-json@6.0.1

## 9.0.0

### Major Changes

- 516859178: `extendNodePath` removed.
- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [516859178]
- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
- Updated dependencies [d999a0801]
  - @pnpm/link-bins@7.0.0
  - @pnpm/types@8.0.0
  - @pnpm/calc-dep-state@2.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/lifecycle@13.0.0
  - @pnpm/read-package-json@6.0.0
  - @pnpm/store-controller-types@13.0.0

## 8.0.3

### Patch Changes

- Updated dependencies [5c525db13]
  - @pnpm/store-controller-types@12.0.0
  - @pnpm/link-bins@6.2.12
  - @pnpm/read-package-json@5.0.12
  - @pnpm/lifecycle@12.1.7

## 8.0.2

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/core-loggers@6.1.4
  - @pnpm/lifecycle@12.1.6
  - @pnpm/link-bins@6.2.11
  - @pnpm/read-package-json@5.0.11
  - @pnpm/store-controller-types@11.0.12

## 8.0.1

### Patch Changes

- Updated dependencies [7ae349cd3]
  - @pnpm/lifecycle@12.1.5

## 8.0.0

### Major Changes

- 1cadc231a: New required option added: `depsStateCache`.

### Patch Changes

- Updated dependencies [1cadc231a]
  - @pnpm/calc-dep-state@1.0.0
  - @pnpm/link-bins@6.2.10

## 7.2.5

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/lifecycle@12.1.4
  - @pnpm/core-loggers@6.1.3
  - @pnpm/link-bins@6.2.9
  - @pnpm/read-package-json@5.0.10
  - @pnpm/store-controller-types@11.0.11

## 7.2.4

### Patch Changes

- ea24c69fe: `@pnpm/logger` should be a peer dependency.

## 7.2.3

### Patch Changes

- Updated dependencies [701ea0746]
- Updated dependencies [b5734a4a7]
  - @pnpm/link-bins@6.2.8
  - @pnpm/types@7.8.0
  - @pnpm/core-loggers@6.1.2
  - @pnpm/lifecycle@12.1.3
  - @pnpm/read-package-json@5.0.9
  - @pnpm/store-controller-types@11.0.10

## 7.2.2

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/core-loggers@6.1.1
  - @pnpm/lifecycle@12.1.2
  - @pnpm/link-bins@6.2.7
  - @pnpm/read-package-json@5.0.8
  - @pnpm/store-controller-types@11.0.9

## 7.2.1

### Patch Changes

- Updated dependencies [ba9b2eba1]
- Updated dependencies [ba9b2eba1]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/types@7.7.0
  - @pnpm/lifecycle@12.1.1
  - @pnpm/link-bins@6.2.6
  - @pnpm/read-package-json@5.0.7
  - @pnpm/store-controller-types@11.0.8

## 7.2.0

### Minor Changes

- 002778559: New setting added: `scriptsPrependNodePath`. This setting can be `true`, `false`, or `warn-only`.
  When `true`, the path to the `node` executable with which pnpm executed is prepended to the `PATH` of the scripts.
  When `warn-only`, pnpm will print a warning if the scripts run with a `node` binary that differs from the `node` binary executing the pnpm CLI.

### Patch Changes

- Updated dependencies [002778559]
  - @pnpm/lifecycle@12.1.0

## 7.1.7

### Patch Changes

- Updated dependencies [bb0f8bc16]
  - @pnpm/link-bins@6.2.5

## 7.1.6

### Patch Changes

- Updated dependencies [302ae4f6f]
- Updated dependencies [fa03cbdc8]
  - @pnpm/types@7.6.0
  - @pnpm/lifecycle@12.0.2
  - @pnpm/core-loggers@6.0.6
  - @pnpm/link-bins@6.2.4
  - @pnpm/read-package-json@5.0.6
  - @pnpm/store-controller-types@11.0.7

## 7.1.5

### Patch Changes

- Updated dependencies [5b90ab98f]
  - @pnpm/lifecycle@12.0.1

## 7.1.4

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [37dcfceeb]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/lifecycle@12.0.0
  - @pnpm/core-loggers@6.0.5
  - @pnpm/link-bins@6.2.3
  - @pnpm/read-package-json@5.0.5
  - @pnpm/store-controller-types@11.0.6

## 7.1.3

### Patch Changes

- Updated dependencies [a916accec]
  - @pnpm/link-bins@6.2.2

## 7.1.2

### Patch Changes

- Updated dependencies [6375cdce0]
  - @pnpm/link-bins@6.2.1

## 7.1.1

### Patch Changes

- Updated dependencies [4a4d42d8f]
  - @pnpm/lifecycle@11.0.5

## 7.1.0

### Minor Changes

- c7081cbb4: New option added: `extendNodePath`. When it is set to `false`, pnpm does not set the `NODE_PATH` environment variable in the command shims.

### Patch Changes

- Updated dependencies [0d4a7c69e]
- Updated dependencies [c7081cbb4]
  - @pnpm/link-bins@6.2.0

## 7.0.10

### Patch Changes

- Updated dependencies [83e23601e]
- Updated dependencies [553a5d840]
  - @pnpm/link-bins@6.1.0

## 7.0.9

### Patch Changes

- @pnpm/link-bins@6.0.8

## 7.0.8

### Patch Changes

- 6208e2a71: Link own binaries of package before running its lifecycle scripts.

## 7.0.7

### Patch Changes

- @pnpm/link-bins@6.0.7

## 7.0.6

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/core-loggers@6.0.4
  - @pnpm/lifecycle@11.0.4
  - @pnpm/link-bins@6.0.6
  - @pnpm/read-package-json@5.0.4
  - @pnpm/store-controller-types@11.0.5

## 7.0.5

### Patch Changes

- Updated dependencies [7af16a011]
  - @pnpm/lifecycle@11.0.3

## 7.0.4

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/core-loggers@6.0.3
  - @pnpm/lifecycle@11.0.2
  - @pnpm/link-bins@6.0.5
  - @pnpm/read-package-json@5.0.3
  - @pnpm/store-controller-types@11.0.4

## 7.0.3

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/core-loggers@6.0.2
  - @pnpm/lifecycle@11.0.1
  - @pnpm/link-bins@6.0.4
  - @pnpm/read-package-json@5.0.2
  - @pnpm/store-controller-types@11.0.3

## 7.0.2

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/link-bins@6.0.3

## 7.0.1

### Patch Changes

- @pnpm/link-bins@6.0.2

## 7.0.0

### Major Changes

- e6a2654a2: `prepare` scripts of Git-hosted packages are not executed (they are executed during fetching by `@pnpm/git-fetcher`).

### Patch Changes

- Updated dependencies [e6a2654a2]
  - @pnpm/lifecycle@11.0.0
  - @pnpm/store-controller-types@11.0.2

## 6.0.1

### Patch Changes

- 1a9b4f812: Fix incorrect return in getSubgraphToBuild
- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/link-bins@6.0.1
  - @pnpm/core-loggers@6.0.1
  - @pnpm/lifecycle@10.0.1
  - @pnpm/read-package-json@5.0.1
  - @pnpm/store-controller-types@11.0.1

## 6.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [6871d74b2]
- Updated dependencies [06c6c9959]
- Updated dependencies [97b986fbc]
- Updated dependencies [90487a3a8]
- Updated dependencies [f2bb5cbeb]
  - @pnpm/constants@5.0.0
  - @pnpm/link-bins@6.0.0
  - @pnpm/core-loggers@6.0.0
  - @pnpm/lifecycle@10.0.0
  - @pnpm/read-package-json@5.0.0
  - @pnpm/store-controller-types@11.0.0
  - @pnpm/types@7.0.0

## 5.2.12

### Patch Changes

- Updated dependencies [d853fb14a]
- Updated dependencies [d853fb14a]
- Updated dependencies [d853fb14a]
  - @pnpm/lifecycle@9.6.5
  - @pnpm/link-bins@5.3.25
  - @pnpm/read-package-json@4.0.0

## 5.2.11

### Patch Changes

- Updated dependencies [6350a3381]
  - @pnpm/link-bins@5.3.24

## 5.2.10

### Patch Changes

- Updated dependencies [8d1dfa89c]
  - @pnpm/store-controller-types@10.0.0

## 5.2.9

### Patch Changes

- Updated dependencies [a78e5c47f]
  - @pnpm/link-bins@5.3.23

## 5.2.8

### Patch Changes

- @pnpm/link-bins@5.3.22

## 5.2.7

### Patch Changes

- Updated dependencies [9a9bc67d2]
  - @pnpm/lifecycle@9.6.4

## 5.2.6

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0
  - @pnpm/core-loggers@5.0.3
  - @pnpm/lifecycle@9.6.3
  - @pnpm/link-bins@5.3.21
  - @pnpm/read-package-json@3.1.9
  - @pnpm/store-controller-types@9.2.1

## 5.2.5

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/store-controller-types@9.2.0

## 5.2.4

### Patch Changes

- @pnpm/link-bins@5.3.20
- @pnpm/read-package-json@3.1.8
- @pnpm/lifecycle@9.6.2

## 5.2.3

### Patch Changes

- @pnpm/link-bins@5.3.19

## 5.2.2

### Patch Changes

- @pnpm/link-bins@5.3.18

## 5.2.1

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/core-loggers@5.0.2
  - @pnpm/lifecycle@9.6.1
  - @pnpm/link-bins@5.3.17
  - @pnpm/read-package-json@3.1.7
  - @pnpm/store-controller-types@9.1.2

## 5.2.0

### Minor Changes

- 50b360ec1: A new option added for specifying the shell to use, when running scripts: scriptShell.

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/lifecycle@9.6.0

## 5.1.2

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
- Updated dependencies [212671848]
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - @pnpm/read-package-json@3.1.6
  - @pnpm/core-loggers@5.0.1
  - @pnpm/lifecycle@9.5.1
  - @pnpm/link-bins@5.3.16
  - @pnpm/store-controller-types@9.1.1

## 5.1.1

### Patch Changes

- Updated dependencies [fb863fae4]
  - @pnpm/link-bins@5.3.15

## 5.1.0

### Minor Changes

- f591fdeeb: New option added: extraEnv. extraEnv allows to pass environment variables that will be set for the child process.

### Patch Changes

- Updated dependencies [f591fdeeb]
- Updated dependencies [f591fdeeb]
  - @pnpm/lifecycle@9.5.0

## 5.0.19

### Patch Changes

- Updated dependencies [51311d3ba]
  - @pnpm/link-bins@5.3.14

## 5.0.18

### Patch Changes

- 203e65ac8: The INIT_CWD env variable is always set to the lockfile directory, for scripts of dependencies.
- Updated dependencies [203e65ac8]
  - @pnpm/lifecycle@9.4.0

## 5.0.17

### Patch Changes

- Updated dependencies [23cf3c88b]
  - @pnpm/lifecycle@9.3.0

## 5.0.16

### Patch Changes

- Updated dependencies [0a6544043]
  - @pnpm/store-controller-types@9.1.0

## 5.0.15

### Patch Changes

- Updated dependencies [86cd72de3]
- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0
  - @pnpm/store-controller-types@9.0.0
  - @pnpm/lifecycle@9.2.5

## 5.0.14

### Patch Changes

- @pnpm/link-bins@5.3.13
- @pnpm/read-package-json@3.1.5
- @pnpm/lifecycle@9.2.4

## 5.0.13

### Patch Changes

- Updated dependencies [9f5803187]
  - @pnpm/read-package-json@3.1.4
  - @pnpm/lifecycle@9.2.3
  - @pnpm/link-bins@5.3.12

## 5.0.12

### Patch Changes

- @pnpm/link-bins@5.3.11

## 5.0.11

### Patch Changes

- @pnpm/link-bins@5.3.10

## 5.0.10

### Patch Changes

- @pnpm/link-bins@5.3.9

## 5.0.9

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - @pnpm/lifecycle@9.2.2
  - @pnpm/link-bins@5.3.8

## 5.0.8

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0
  - @pnpm/lifecycle@9.2.1

## 5.0.7

### Patch Changes

- Updated dependencies [76aaead32]
  - @pnpm/lifecycle@9.2.0

## 5.0.6

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2
  - @pnpm/lifecycle@9.1.3
  - @pnpm/link-bins@5.3.7
  - @pnpm/read-package-json@3.1.3
  - @pnpm/store-controller-types@8.0.2

## 5.0.5

### Patch Changes

- @pnpm/link-bins@5.3.6

## 5.0.4

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [e1ca9fc13]
  - @pnpm/types@6.1.0
  - @pnpm/link-bins@5.3.5
  - @pnpm/core-loggers@4.1.1
  - @pnpm/lifecycle@9.1.2
  - @pnpm/read-package-json@3.1.2
  - @pnpm/store-controller-types@8.0.1

## 5.0.3

### Patch Changes

- @pnpm/link-bins@5.3.4

## 5.0.2

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
- Updated dependencies [68d8dc68f]
  - @pnpm/lifecycle@9.1.1
  - @pnpm/core-loggers@4.1.0

## 5.0.1

### Patch Changes

- Updated dependencies [8094b2a62]
  - @pnpm/lifecycle@9.1.0

## 5.0.0

### Major Changes

- bb59db642: `peripheralLocation` in `DependenciesGraphNode` renamed to `dir`.
- a5febb913: The upload function of the store controller accepts `opts.filesIndexFile` instead of `opts.packageId`.
- e3990787a: Rename NodeModules to Modules in option names.

### Minor Changes

- 9b1b520d9: `packageId` removed from `DependenciesGraphNode`.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [f516d266c]
- Updated dependencies [da091c711]
- Updated dependencies [f35a3ec1c]
- Updated dependencies [42e6490d1]
- Updated dependencies [a5febb913]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [4f5801b1c]
- Updated dependencies [a5febb913]
- Updated dependencies [e3990787a]
  - @pnpm/constants@4.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lifecycle@9.0.0
  - @pnpm/core-loggers@4.0.2
  - @pnpm/link-bins@5.3.3
  - @pnpm/read-package-json@3.1.1

## 5.0.0-alpha.5

### Major Changes

- a5febb913: The upload function of the store controller accepts `opts.filesIndexFile` instead of `opts.packageId`.

### Patch Changes

- Updated dependencies [ca9f50844]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/store-controller-types@8.0.0-alpha.4

## 5.0.0-alpha.4

### Major Changes

- e3990787: Rename NodeModules to Modules in option names.

### Patch Changes

- Updated dependencies [da091c71]
- Updated dependencies [e3990787]
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/lifecycle@9.0.0-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0
  - @pnpm/link-bins@5.3.3-alpha.0
  - @pnpm/read-package-json@3.1.1-alpha.0

## 4.1.15-alpha.3

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0

## 4.1.14-alpha.2

### Patch Changes

- Updated dependencies [f35a3ec1c]
- Updated dependencies [42e6490d1]
  - @pnpm/lifecycle@8.2.0-alpha.0
  - @pnpm/store-controller-types@8.0.0-alpha.2

## 4.1.14-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1

## 4.1.14-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0

## 4.1.14

### Patch Changes

- Updated dependencies [2ec4c4eb9]
  - @pnpm/lifecycle@8.2.0

## 4.1.13

### Patch Changes

- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/link-bins@5.3.2
