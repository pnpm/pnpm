# @pnpm/hoist

## 6.2.12

### Patch Changes

- Updated dependencies [5eb41a551]
  - @pnpm/link-bins@7.2.8

## 6.2.11

### Patch Changes

- Updated dependencies [abb41a626]
- Updated dependencies [d665f3ff7]
  - @pnpm/matcher@3.2.0
  - @pnpm/types@8.7.0
  - dependency-path@9.2.6
  - @pnpm/link-bins@7.2.7
  - @pnpm/lockfile-types@4.3.3
  - @pnpm/lockfile-utils@4.2.6
  - @pnpm/lockfile-walker@5.0.15
  - @pnpm/symlink-dependency@5.0.9

## 6.2.10

### Patch Changes

- Updated dependencies [156cc1ef6]
- Updated dependencies [9b44d38a4]
  - @pnpm/types@8.6.0
  - @pnpm/matcher@3.1.0
  - dependency-path@9.2.5
  - @pnpm/link-bins@7.2.6
  - @pnpm/lockfile-types@4.3.2
  - @pnpm/lockfile-utils@4.2.5
  - @pnpm/lockfile-walker@5.0.14
  - @pnpm/symlink-dependency@5.0.8

## 6.2.9

### Patch Changes

- Updated dependencies [e3b5137d1]
  - @pnpm/symlink-dependency@5.0.7

## 6.2.8

### Patch Changes

- Updated dependencies [07bc24ad1]
  - @pnpm/link-bins@7.2.5

## 6.2.7

### Patch Changes

- @pnpm/lockfile-utils@4.2.4
- @pnpm/link-bins@7.2.4

## 6.2.6

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [8103f92bd]
  - @pnpm/link-bins@7.2.4
  - @pnpm/lockfile-utils@4.2.3
  - @pnpm/lockfile-walker@5.0.13

## 6.2.5

### Patch Changes

- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - dependency-path@9.2.4
  - @pnpm/link-bins@7.2.3
  - @pnpm/lockfile-types@4.3.1
  - @pnpm/lockfile-utils@4.2.2
  - @pnpm/lockfile-walker@5.0.12
  - @pnpm/symlink-dependency@5.0.6

## 6.2.4

### Patch Changes

- Updated dependencies [c83f40c10]
  - @pnpm/lockfile-utils@4.2.1

## 6.2.3

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-types@4.3.0
  - @pnpm/lockfile-utils@4.2.0
  - @pnpm/lockfile-walker@5.0.11
  - @pnpm/link-bins@7.2.2

## 6.2.2

### Patch Changes

- @pnpm/link-bins@7.2.2

## 6.2.1

### Patch Changes

- Updated dependencies [e3f4d131c]
  - @pnpm/lockfile-utils@4.1.0
  - @pnpm/link-bins@7.2.1

## 6.2.0

### Minor Changes

- 28f000509: A new setting supported: `prefer-symlinked-executables`. When `true`, pnpm will create symlinks to executables in
  `node_modules/.bin` instead of command shims (but on POSIX systems only).

  This setting is `true` by default when `node-linker` is set to `hoisted`.

  Related issue: [#4782](https://github.com/pnpm/pnpm/issues/4782).

### Patch Changes

- Updated dependencies [28f000509]
  - @pnpm/link-bins@7.2.0

## 6.1.9

### Patch Changes

- @pnpm/link-bins@7.1.7
- dependency-path@9.2.3
- @pnpm/lockfile-utils@4.0.10
- @pnpm/lockfile-walker@5.0.10

## 6.1.8

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/link-bins@7.1.6
  - @pnpm/lockfile-utils@4.0.9
  - @pnpm/lockfile-walker@5.0.9

## 6.1.7

### Patch Changes

- Updated dependencies [fc581d371]
  - dependency-path@9.2.2
  - @pnpm/lockfile-utils@4.0.8
  - @pnpm/lockfile-walker@5.0.8

## 6.1.6

### Patch Changes

- Updated dependencies [d01c32355]
- Updated dependencies [8e5b77ef6]
- Updated dependencies [8e5b77ef6]
  - @pnpm/lockfile-types@4.2.0
  - @pnpm/types@8.4.0
  - @pnpm/lockfile-utils@4.0.7
  - @pnpm/lockfile-walker@5.0.7
  - dependency-path@9.2.1
  - @pnpm/link-bins@7.1.5
  - @pnpm/symlink-dependency@5.0.5

## 6.1.5

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [c635f9fc1]
  - @pnpm/types@8.3.0
  - @pnpm/lockfile-types@4.1.0
  - dependency-path@9.2.0
  - @pnpm/link-bins@7.1.4
  - @pnpm/lockfile-utils@4.0.6
  - @pnpm/lockfile-walker@5.0.6
  - @pnpm/symlink-dependency@5.0.4

## 6.1.4

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [725636a90]
  - @pnpm/types@8.2.0
  - dependency-path@9.1.4
  - @pnpm/link-bins@7.1.3
  - @pnpm/lockfile-types@4.0.3
  - @pnpm/lockfile-utils@4.0.5
  - @pnpm/lockfile-walker@5.0.5
  - @pnpm/symlink-dependency@5.0.3

## 6.1.3

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - dependency-path@9.1.3
  - @pnpm/link-bins@7.1.2
  - @pnpm/lockfile-types@4.0.2
  - @pnpm/lockfile-utils@4.0.4
  - @pnpm/lockfile-walker@5.0.4
  - @pnpm/symlink-dependency@5.0.2

## 6.1.2

### Patch Changes

- Updated dependencies [c57695550]
  - dependency-path@9.1.2
  - @pnpm/lockfile-utils@4.0.3
  - @pnpm/lockfile-walker@5.0.3

## 6.1.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - dependency-path@9.1.1
  - @pnpm/link-bins@7.1.1
  - @pnpm/lockfile-types@4.0.1
  - @pnpm/lockfile-utils@4.0.2
  - @pnpm/lockfile-walker@5.0.2
  - @pnpm/symlink-dependency@5.0.1

## 6.1.0

### Minor Changes

- 8fa95fd86: New option added: `extraNodePaths`.

### Patch Changes

- Updated dependencies [0a70aedb1]
- Updated dependencies [8fa95fd86]
- Updated dependencies [688b0eaff]
- Updated dependencies [1267e4eff]
  - dependency-path@9.1.0
  - @pnpm/link-bins@7.1.0
  - @pnpm/lockfile-utils@4.0.1
  - @pnpm/constants@6.1.0
  - @pnpm/lockfile-walker@5.0.1

## 6.0.0

### Major Changes

- 516859178: `extendNodePath` removed.
- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [516859178]
- Updated dependencies [d504dc380]
- Updated dependencies [faf830b8f]
- Updated dependencies [542014839]
  - @pnpm/link-bins@7.0.0
  - @pnpm/types@8.0.0
  - dependency-path@9.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/lockfile-types@4.0.0
  - @pnpm/lockfile-utils@4.0.0
  - @pnpm/lockfile-walker@5.0.0
  - @pnpm/matcher@3.0.0
  - @pnpm/symlink-dependency@5.0.0

## 5.2.15

### Patch Changes

- @pnpm/link-bins@6.2.12

## 5.2.14

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-types@3.2.0
  - @pnpm/types@7.10.0
  - @pnpm/lockfile-utils@3.2.1
  - @pnpm/lockfile-walker@4.0.15
  - dependency-path@8.0.11
  - @pnpm/link-bins@6.2.11
  - @pnpm/symlink-dependency@4.0.13

## 5.2.13

### Patch Changes

- Updated dependencies [cdc521cfa]
  - @pnpm/lockfile-utils@3.2.0
  - @pnpm/link-bins@6.2.10

## 5.2.12

### Patch Changes

- @pnpm/link-bins@6.2.10

## 5.2.11

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - dependency-path@8.0.10
  - @pnpm/link-bins@6.2.9
  - @pnpm/lockfile-types@3.1.5
  - @pnpm/lockfile-utils@3.1.6
  - @pnpm/lockfile-walker@4.0.14
  - @pnpm/symlink-dependency@4.0.12

## 5.2.10

### Patch Changes

- Updated dependencies [701ea0746]
- Updated dependencies [b5734a4a7]
  - @pnpm/link-bins@6.2.8
  - @pnpm/types@7.8.0
  - dependency-path@8.0.9
  - @pnpm/lockfile-types@3.1.4
  - @pnpm/lockfile-utils@3.1.5
  - @pnpm/lockfile-walker@4.0.13
  - @pnpm/symlink-dependency@4.0.11

## 5.2.9

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - dependency-path@8.0.8
  - @pnpm/link-bins@6.2.7
  - @pnpm/lockfile-types@3.1.3
  - @pnpm/lockfile-utils@3.1.4
  - @pnpm/lockfile-walker@4.0.12
  - @pnpm/symlink-dependency@4.0.10

## 5.2.8

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/symlink-dependency@4.0.9
  - dependency-path@8.0.7
  - @pnpm/link-bins@6.2.6
  - @pnpm/lockfile-types@3.1.2
  - @pnpm/lockfile-utils@3.1.3
  - @pnpm/lockfile-walker@4.0.11

## 5.2.7

### Patch Changes

- Updated dependencies [3cf543fc1]
  - @pnpm/lockfile-utils@3.1.2

## 5.2.6

### Patch Changes

- Updated dependencies [631877ebf]
  - @pnpm/symlink-dependency@4.0.8

## 5.2.5

### Patch Changes

- Updated dependencies [bb0f8bc16]
  - @pnpm/link-bins@6.2.5

## 5.2.4

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - dependency-path@8.0.6
  - @pnpm/link-bins@6.2.4
  - @pnpm/lockfile-types@3.1.1
  - @pnpm/lockfile-utils@3.1.1
  - @pnpm/lockfile-walker@4.0.10
  - @pnpm/symlink-dependency@4.0.7

## 5.2.3

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/lockfile-types@3.1.0
  - @pnpm/lockfile-utils@3.1.0
  - dependency-path@8.0.5
  - @pnpm/link-bins@6.2.3
  - @pnpm/lockfile-walker@4.0.9
  - @pnpm/symlink-dependency@4.0.6

## 5.2.2

### Patch Changes

- Updated dependencies [a916accec]
  - @pnpm/link-bins@6.2.2

## 5.2.1

### Patch Changes

- Updated dependencies [6375cdce0]
  - @pnpm/link-bins@6.2.1

## 5.2.0

### Minor Changes

- 59a4152ce: allow to hoist packages based on importerIds, only hoist packages that are subdependencies of the specified importerIds

## 5.1.0

### Minor Changes

- c7081cbb4: New option added: `extendNodePath`. When it is set to `false`, pnpm does not set the `NODE_PATH` environment variable in the command shims.

### Patch Changes

- Updated dependencies [0d4a7c69e]
- Updated dependencies [c7081cbb4]
  - @pnpm/link-bins@6.2.0

## 5.0.14

### Patch Changes

- Updated dependencies [83e23601e]
- Updated dependencies [553a5d840]
  - @pnpm/link-bins@6.1.0

## 5.0.13

### Patch Changes

- @pnpm/link-bins@6.0.8

## 5.0.12

### Patch Changes

- @pnpm/link-bins@6.0.7

## 5.0.11

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - dependency-path@8.0.4
  - @pnpm/link-bins@6.0.6
  - @pnpm/lockfile-utils@3.0.8
  - @pnpm/lockfile-walker@4.0.8
  - @pnpm/symlink-dependency@4.0.5

## 5.0.10

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - dependency-path@8.0.3
  - @pnpm/link-bins@6.0.5
  - @pnpm/lockfile-utils@3.0.7
  - @pnpm/lockfile-walker@4.0.7
  - @pnpm/symlink-dependency@4.0.4

## 5.0.9

### Patch Changes

- Updated dependencies [6c418943c]
  - dependency-path@8.0.2
  - @pnpm/lockfile-utils@3.0.6
  - @pnpm/lockfile-walker@4.0.6

## 5.0.8

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - dependency-path@8.0.1
  - @pnpm/link-bins@6.0.4
  - @pnpm/lockfile-utils@3.0.5
  - @pnpm/lockfile-walker@4.0.5
  - @pnpm/symlink-dependency@4.0.3

## 5.0.7

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/link-bins@6.0.3
  - @pnpm/lockfile-utils@3.0.4
  - @pnpm/lockfile-walker@4.0.4

## 5.0.6

### Patch Changes

- 0560ca63f: Do not print a warning if a skipped optional dependency cannot be hoisted.

## 5.0.5

### Patch Changes

- ec097f4ed: Ignore the case of the package name when deciding which dependency to hoist.
- Updated dependencies [20e2f235d]
  - dependency-path@8.0.0
  - @pnpm/lockfile-utils@3.0.3
  - @pnpm/lockfile-walker@4.0.3

## 5.0.4

### Patch Changes

- @pnpm/link-bins@6.0.2

## 5.0.3

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0
  - @pnpm/link-bins@6.0.1
  - dependency-path@7.0.1
  - @pnpm/lockfile-utils@3.0.2
  - @pnpm/lockfile-walker@4.0.2
  - @pnpm/symlink-dependency@4.0.2

## 5.0.2

### Patch Changes

- Updated dependencies [6f198457d]
  - @pnpm/symlink-dependency@4.0.1

## 5.0.1

### Patch Changes

- Updated dependencies [9ceab68f0]
  - dependency-path@7.0.0
  - @pnpm/lockfile-utils@3.0.1
  - @pnpm/lockfile-walker@4.0.1

## 5.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [6871d74b2]
- Updated dependencies [06c6c9959]
- Updated dependencies [97b986fbc]
- Updated dependencies [6871d74b2]
- Updated dependencies [e4efddbd2]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [f2bb5cbeb]
  - @pnpm/constants@5.0.0
  - @pnpm/link-bins@6.0.0
  - dependency-path@6.0.0
  - @pnpm/lockfile-types@3.0.0
  - @pnpm/lockfile-utils@3.0.0
  - @pnpm/lockfile-walker@4.0.0
  - @pnpm/matcher@2.0.0
  - @pnpm/symlink-dependency@4.0.0
  - @pnpm/types@7.0.0

## 4.0.26

### Patch Changes

- Updated dependencies [d853fb14a]
  - @pnpm/link-bins@5.3.25

## 4.0.25

### Patch Changes

- Updated dependencies [6350a3381]
  - @pnpm/link-bins@5.3.24

## 4.0.24

### Patch Changes

- Updated dependencies [a78e5c47f]
  - @pnpm/link-bins@5.3.23

## 4.0.23

### Patch Changes

- @pnpm/link-bins@5.3.22

## 4.0.22

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-types@2.2.0
  - @pnpm/types@6.4.0
  - @pnpm/lockfile-utils@2.0.22
  - @pnpm/lockfile-walker@3.0.9
  - dependency-path@5.1.1
  - @pnpm/link-bins@5.3.21
  - @pnpm/symlink-dependency@3.0.13

## 4.0.21

### Patch Changes

- Updated dependencies [e27dcf0dc]
  - dependency-path@5.1.0
  - @pnpm/lockfile-utils@2.0.21
  - @pnpm/lockfile-walker@3.0.8

## 4.0.20

### Patch Changes

- @pnpm/lockfile-utils@2.0.20

## 4.0.19

### Patch Changes

- @pnpm/link-bins@5.3.20

## 4.0.18

### Patch Changes

- @pnpm/link-bins@5.3.19

## 4.0.17

### Patch Changes

- Updated dependencies [39142e2ad]
  - dependency-path@5.0.6
  - @pnpm/lockfile-utils@2.0.19
  - @pnpm/lockfile-walker@3.0.7
  - @pnpm/link-bins@5.3.18

## 4.0.16

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/lockfile-types@2.1.1
  - @pnpm/types@6.3.1
  - @pnpm/lockfile-utils@2.0.18
  - @pnpm/lockfile-walker@3.0.6
  - dependency-path@5.0.5
  - @pnpm/link-bins@5.3.17
  - @pnpm/symlink-dependency@3.0.12

## 4.0.15

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/lockfile-types@2.1.0
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - @pnpm/lockfile-utils@2.0.17
  - @pnpm/lockfile-walker@3.0.5
  - dependency-path@5.0.4
  - @pnpm/link-bins@5.3.16
  - @pnpm/symlink-dependency@3.0.11

## 4.0.14

### Patch Changes

- Updated dependencies [fb863fae4]
  - @pnpm/link-bins@5.3.15

## 4.0.13

### Patch Changes

- Updated dependencies [51311d3ba]
  - @pnpm/link-bins@5.3.14

## 4.0.12

### Patch Changes

- @pnpm/symlink-dependency@3.0.10

## 4.0.11

### Patch Changes

- 968c26470: Report an info log instead of a warning when some binaries cannot be linked.

## 4.0.10

### Patch Changes

- @pnpm/link-bins@5.3.13

## 4.0.9

### Patch Changes

- @pnpm/link-bins@5.3.12

## 4.0.8

### Patch Changes

- @pnpm/link-bins@5.3.11

## 4.0.7

### Patch Changes

- @pnpm/link-bins@5.3.10

## 4.0.6

### Patch Changes

- @pnpm/link-bins@5.3.9

## 4.0.5

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [1140ef721]
- Updated dependencies [a2ef8084f]
  - @pnpm/lockfile-utils@2.0.16
  - dependency-path@5.0.3
  - @pnpm/lockfile-walker@3.0.4
  - @pnpm/link-bins@5.3.8

## 4.0.4

### Patch Changes

- @pnpm/symlink-dependency@3.0.9

## 4.0.3

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0
  - dependency-path@5.0.2
  - @pnpm/link-bins@5.3.7
  - @pnpm/lockfile-utils@2.0.15
  - @pnpm/lockfile-walker@3.0.3
  - @pnpm/symlink-dependency@3.0.8

## 4.0.2

### Patch Changes

- @pnpm/link-bins@5.3.6

## 4.0.1

### Patch Changes

- 0a2f3ecc6: Hoisting should not fail if some of the aliases cannot be hoisted due to issues with the lockfile.

## 4.0.0

### Major Changes

- 71a8c8ce3: Breaking changes in the API.

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [e1ca9fc13]
  - @pnpm/types@6.1.0
  - @pnpm/matcher@1.0.3
  - @pnpm/link-bins@5.3.5
  - dependency-path@5.0.1
  - @pnpm/lockfile-utils@2.0.14
  - @pnpm/lockfile-walker@3.0.2
  - @pnpm/symlink-dependency@3.0.7

## 3.0.2

### Patch Changes

- Updated dependencies [41d92948b]
  - dependency-path@5.0.0
  - @pnpm/lockfile-utils@2.0.13
  - @pnpm/lockfile-walker@3.0.1
  - @pnpm/link-bins@5.3.4

## 3.0.1

### Patch Changes

- @pnpm/symlink-dependency@3.0.6

## 3.0.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- 802d145fc: Remove `independent-leaves` support.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [142f8caf7]
- Updated dependencies [da091c711]
- Updated dependencies [6a8a97eee]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/lockfile-walker@3.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-types@2.0.1
  - dependency-path@4.0.7
  - @pnpm/link-bins@5.3.3
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/symlink-dependency@3.0.5

## 3.0.0-alpha.2

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [ca9f50844]
- Updated dependencies [6a8a97eee]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/lockfile-types@2.0.1-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.1
  - @pnpm/lockfile-walker@2.0.3-alpha.1

## 3.0.0-alpha.1

### Major Changes

- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/link-bins@5.3.3-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/lockfile-walker@2.0.3-alpha.0
  - @pnpm/symlink-dependency@3.0.5-alpha.0

## 3.0.0-alpha.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0

## 2.2.3

### Patch Changes

- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/symlink-dependency@3.0.4
  - @pnpm/link-bins@5.3.2
  - @pnpm/lockfile-utils@2.0.11
