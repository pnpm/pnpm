# @pnpm/hoist

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
