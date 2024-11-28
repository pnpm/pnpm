# @pnpm/plugin-commands-store-inspecting

## 1.0.0

### Major Changes

- 099e6af: Changed the structure of the index files in the store to store side effects cache information more efficiently. In the new version, side effects do not list all the files of the package but just the differences [#8636](https://github.com/pnpm/pnpm/pull/8636).

### Minor Changes

- d433cb9: Some registries allow identical content to be published under different package names or versions. To accommodate this, index files in the store are now stored using both the content hash and package identifier.

  This approach ensures that we can:

  1. Validate that the integrity in the lockfile corresponds to the correct package,
     which might not be the case after a poorly resolved Git conflict.
  2. Allow the same content to be referenced by different packages or different versions of the same package.

  Related PR: [#8510](https://github.com/pnpm/pnpm/pull/8510)
  Related issue: [#8204](https://github.com/pnpm/pnpm/issues/8204)

### Patch Changes

- 39c5385: Some commands should ignore the `packageManager` field check of `package.json` [#7959](https://github.com/pnpm/pnpm/issues/7959).
- Updated dependencies [477e0c1]
- Updated dependencies [dfcf034]
- Updated dependencies [592e2ef]
- Updated dependencies [19d5b51]
- Updated dependencies [d433cb9]
- Updated dependencies [1dbc56a]
- Updated dependencies [099e6af]
- Updated dependencies [e9985b6]
  - @pnpm/config@22.0.0
  - @pnpm/store.cafs@5.0.0
  - @pnpm/error@6.0.3
  - @pnpm/store-path@9.0.3
  - @pnpm/client@11.1.13

## 0.2.24

### Patch Changes

- Updated dependencies [a1f4df2]
  - @pnpm/store.cafs@4.0.2
  - @pnpm/client@11.1.12
  - @pnpm/config@21.8.5

## 0.2.23

### Patch Changes

- Updated dependencies [db7ff76]
  - @pnpm/store.cafs@4.0.1
  - @pnpm/client@11.1.11
  - @pnpm/config@21.8.4

## 0.2.22

### Patch Changes

- @pnpm/config@21.8.4
- @pnpm/error@6.0.2
- @pnpm/store-path@9.0.2
- @pnpm/client@11.1.10

## 0.2.21

### Patch Changes

- Updated dependencies [d500d9f]
- Updated dependencies [db420ab]
  - @pnpm/types@12.2.0
  - @pnpm/store.cafs@4.0.0
  - @pnpm/config@21.8.3
  - @pnpm/pick-registry-for-package@6.0.7
  - @pnpm/lockfile.types@1.0.3
  - @pnpm/client@11.1.9

## 0.2.20

### Patch Changes

- Updated dependencies [7ee59a1]
  - @pnpm/types@12.1.0
  - @pnpm/config@21.8.2
  - @pnpm/pick-registry-for-package@6.0.6
  - @pnpm/lockfile.types@1.0.2
  - @pnpm/client@11.1.8
  - @pnpm/store.cafs@3.0.8

## 0.2.19

### Patch Changes

- Updated dependencies [251ab21]
  - @pnpm/config@21.8.1

## 0.2.18

### Patch Changes

- Updated dependencies [26b065c]
  - @pnpm/config@21.8.0

## 0.2.17

### Patch Changes

- Updated dependencies [cb006df]
- Updated dependencies [d20eed3]
  - @pnpm/lockfile.types@1.0.1
  - @pnpm/types@12.0.0
  - @pnpm/config@21.7.0
  - @pnpm/pick-registry-for-package@6.0.5
  - @pnpm/client@11.1.7
  - @pnpm/store.cafs@3.0.7

## 0.2.16

### Patch Changes

- Updated dependencies [797ef0f]
  - @pnpm/lockfile.types@1.0.0
  - @pnpm/config@21.6.3
  - @pnpm/client@11.1.6

## 0.2.15

### Patch Changes

- Updated dependencies [0ef168b]
  - @pnpm/types@11.1.0
  - @pnpm/config@21.6.2
  - @pnpm/pick-registry-for-package@6.0.4
  - @pnpm/lockfile-types@7.1.3
  - @pnpm/client@11.1.5
  - @pnpm/store.cafs@3.0.6

## 0.2.14

### Patch Changes

- Updated dependencies [afe520d]
- Updated dependencies [afe520d]
  - @pnpm/store.cafs@3.0.5
  - @pnpm/config@21.6.1
  - @pnpm/client@11.1.4

## 0.2.13

### Patch Changes

- Updated dependencies [1b03682]
- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/config@21.6.0
  - @pnpm/types@11.0.0
  - @pnpm/client@11.1.3
  - @pnpm/pick-registry-for-package@6.0.3
  - @pnpm/lockfile-types@7.1.2
  - @pnpm/store.cafs@3.0.4

## 0.2.12

### Patch Changes

- Updated dependencies [7c6c923]
- Updated dependencies [7d10394]
- Updated dependencies [d8eab39]
- Updated dependencies [13e55b2]
- Updated dependencies [04b8363]
  - @pnpm/config@21.5.0
  - @pnpm/types@10.1.1
  - @pnpm/pick-registry-for-package@6.0.2
  - @pnpm/lockfile-types@7.1.1
  - @pnpm/client@11.1.2
  - @pnpm/store.cafs@3.0.3

## 0.2.11

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/lockfile-types@7.1.0
  - @pnpm/config@21.4.0

## 0.2.10

### Patch Changes

- Updated dependencies [b7ca13f]
  - @pnpm/config@21.3.0
  - @pnpm/client@11.1.1

## 0.2.9

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/client@11.1.0
  - @pnpm/store.cafs@3.0.2
  - @pnpm/config@21.2.3

## 0.2.8

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-types@7.0.0
  - @pnpm/config@21.2.2
  - @pnpm/pick-registry-for-package@6.0.1
  - @pnpm/client@11.0.6
  - @pnpm/store.cafs@3.0.1

## 0.2.7

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/config@21.2.1
  - @pnpm/store-path@9.0.1
  - @pnpm/client@11.0.5

## 0.2.6

### Patch Changes

- @pnpm/client@11.0.4

## 0.2.5

### Patch Changes

- @pnpm/client@11.0.3

## 0.2.4

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/config@21.2.0

## 0.2.3

### Patch Changes

- @pnpm/client@11.0.2

## 0.2.2

### Patch Changes

- @pnpm/client@11.0.1

## 0.2.1

### Patch Changes

- Updated dependencies [e0f47f4]
  - @pnpm/config@21.1.0

## 0.2.0

### Minor Changes

- 7733f3a: Added support for registry-scoped SSL configurations (cert, key, and ca). Three new settings supported: `<registryURL>:certfile`, `<registryURL>:keyfile`, and `<registryURL>:ca`. For instance:

  ```
  //registry.mycomp.com/:certfile=server-cert.pem
  //registry.mycomp.com/:keyfile=server-key.pem
  //registry.mycomp.com/:cafile=client-cert.pem
  ```

  Related issue: [#7427](https://github.com/pnpm/pnpm/issues/7427).
  Related PR: [#7626](https://github.com/pnpm/pnpm/pull/7626).

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
- Updated dependencies [6cdbf11]
- Updated dependencies [2d9e3b8]
- Updated dependencies [36dcaa0]
- Updated dependencies [086b69c]
- Updated dependencies [cfa33f1]
- Updated dependencies [e748162]
- Updated dependencies [2b89155]
- Updated dependencies [27a96a8]
- Updated dependencies [60839fc]
- Updated dependencies [730929e]
- Updated dependencies [98566d9]
  - @pnpm/client@11.0.0
  - @pnpm/types@10.0.0
  - @pnpm/config@21.0.0
  - @pnpm/error@6.0.0
  - @pnpm/pick-registry-for-package@6.0.0
  - @pnpm/parse-wanted-dependency@6.0.0
  - @pnpm/lockfile-types@6.0.0
  - @pnpm/store-path@9.0.0
  - @pnpm/graceful-fs@4.0.0
  - @pnpm/store.cafs@3.0.0

## 0.1.5

### Patch Changes

- @pnpm/store.cafs@2.0.12
- @pnpm/client@10.0.46
- @pnpm/config@20.4.2

## 0.1.4

### Patch Changes

- @pnpm/client@10.0.45

## 0.1.3

### Patch Changes

- Updated dependencies [37ccff637]
- Updated dependencies [d9564e354]
  - @pnpm/store-path@8.0.2
  - @pnpm/config@20.4.1
  - @pnpm/client@10.0.44

## 0.1.2

### Patch Changes

- @pnpm/client@10.0.43

## 0.1.1

### Patch Changes

- 459945292: The package information output by cat-index should be sorted by key.
- Updated dependencies [c597f72ec]
  - @pnpm/config@20.4.0

## 0.1.0

### Minor Changes

- 97b450e1f: New commands added for inspecting the store:

  - **pnpm cat-index**: Prints the index file of a specific package in the store. The package is specified by its name and version: `pnpm cat-index <pkg name>@<pkg version>`
  - **pnpm cat-file**: Prints the contents of a file based on the hash value stored in the index file. For example:
    ```
    pnpm cat-file sha512-mvavhfVcEREI7d8dfvfvIkuBLnx7+rrkHHnPi8mpEDUlNpY4CUY+CvJ5mrrLl18iQYo1odFwBV7z/cOypG7xxQ==
    ```
  - **pnpm find-hash**: Lists the packages that include the file with the specified hash. For example:
    ```
    pnpm find-hash sha512-mvavhfVcEREI7d8dfvfvIkuBLnx7+rrkHHnPi8mpEDUlNpY4CUY+CvJ5mrrLl18iQYo1odFwBV7z/cOypG7xxQ==
    ```
    This command is **experimental**. We might change how it behaves.

  Related issue: [#7413](https://github.com/pnpm/pnpm/issues/7413).

### Patch Changes

- Updated dependencies [4e71066dd]
- Updated dependencies [33313d2fd]
- Updated dependencies [4d34684f1]
  - @pnpm/config@20.3.0
  - @pnpm/store.cafs@2.0.11
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/types@9.4.2
  - @pnpm/pick-registry-for-package@5.0.6
  - @pnpm/client@10.0.42
