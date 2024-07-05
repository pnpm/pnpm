# @pnpm/store-connection-manager

## 8.3.1

### Patch Changes

- Updated dependencies [1b03682]
  - @pnpm/config@21.6.0
  - @pnpm/client@11.1.3
  - @pnpm/package-store@20.3.1
  - @pnpm/cli-meta@6.0.3
  - @pnpm/server@18.2.2

## 8.3.0

### Minor Changes

- 7c6c923: Some registries allow the exact same content to be published under different package names and/or versions. This breaks the validity checks of packages in the store. To avoid errors when verifying the names and versions of such packages in the store, you may now set the `strict-store-pkg-content-check` setting to `false` [#4724](https://github.com/pnpm/pnpm/issues/4724).

### Patch Changes

- Updated dependencies [7c6c923]
- Updated dependencies [7d10394]
- Updated dependencies [d8eab39]
- Updated dependencies [04b8363]
  - @pnpm/package-store@20.3.0
  - @pnpm/config@21.5.0
  - @pnpm/server@18.2.1
  - @pnpm/cli-meta@6.0.2
  - @pnpm/client@11.1.2

## 8.2.2

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/config@21.4.0
  - @pnpm/package-store@20.2.1
  - @pnpm/server@18.2.0

## 8.2.1

### Patch Changes

- Updated dependencies [b7ca13f]
  - @pnpm/config@21.3.0
  - @pnpm/client@11.1.1
  - @pnpm/package-store@20.2.0
  - @pnpm/server@18.2.0

## 8.2.0

### Minor Changes

- 0c08e1c: Added a new function for clearing resolution cache.

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/package-store@20.2.0
  - @pnpm/client@11.1.0
  - @pnpm/server@18.2.0
  - @pnpm/config@21.2.3

## 8.1.4

### Patch Changes

- @pnpm/cli-meta@6.0.1
- @pnpm/config@21.2.2
- @pnpm/client@11.0.6
- @pnpm/package-store@20.1.2
- @pnpm/server@18.1.1

## 8.1.3

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/config@21.2.1
  - @pnpm/store-path@9.0.1
  - @pnpm/client@11.0.5
  - @pnpm/package-store@20.1.1
  - @pnpm/server@18.1.0

## 8.1.2

### Patch Changes

- @pnpm/client@11.0.4
- @pnpm/package-store@20.1.0
- @pnpm/server@18.1.0

## 8.1.1

### Patch Changes

- @pnpm/client@11.0.3
- @pnpm/package-store@20.1.0
- @pnpm/server@18.1.0

## 8.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/package-store@20.1.0
  - @pnpm/config@21.2.0
  - @pnpm/server@18.1.0

## 8.0.4

### Patch Changes

- @pnpm/client@11.0.2
- @pnpm/package-store@20.0.1
- @pnpm/server@18.0.0

## 8.0.3

### Patch Changes

- @pnpm/package-store@20.0.1
- @pnpm/server@18.0.0

## 8.0.2

### Patch Changes

- @pnpm/client@11.0.1
- @pnpm/package-store@20.0.0
- @pnpm/server@18.0.0

## 8.0.1

### Patch Changes

- Updated dependencies [e0f47f4]
  - @pnpm/config@21.1.0

## 8.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- 7733f3a: Added support for registry-scoped SSL configurations (cert, key, and ca). Three new settings supported: `<registryURL>:certfile`, `<registryURL>:keyfile`, and `<registryURL>:ca`. For instance:

  ```
  //registry.mycomp.com/:certfile=server-cert.pem
  //registry.mycomp.com/:keyfile=server-key.pem
  //registry.mycomp.com/:cafile=client-cert.pem
  ```

  Related issue: [#7427](https://github.com/pnpm/pnpm/issues/7427).
  Related PR: [#7626](https://github.com/pnpm/pnpm/pull/7626).

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [cdd8365]
- Updated dependencies [43cdd87]
- Updated dependencies [2d9e3b8]
- Updated dependencies [cfa33f1]
- Updated dependencies [e748162]
- Updated dependencies [2b89155]
- Updated dependencies [60839fc]
- Updated dependencies [730929e]
- Updated dependencies [98566d9]
  - @pnpm/client@11.0.0
  - @pnpm/config@21.0.0
  - @pnpm/error@6.0.0
  - @pnpm/server@18.0.0
  - @pnpm/package-store@20.0.0
  - @pnpm/store-path@9.0.0
  - @pnpm/cli-meta@6.0.0

## 7.0.26

### Patch Changes

- @pnpm/package-store@19.0.15
- @pnpm/server@17.0.7
- @pnpm/client@10.0.46
- @pnpm/config@20.4.2

## 7.0.25

### Patch Changes

- @pnpm/client@10.0.45
- @pnpm/package-store@19.0.14
- @pnpm/server@17.0.6

## 7.0.24

### Patch Changes

- Updated dependencies [37ccff637]
- Updated dependencies [d9564e354]
  - @pnpm/store-path@8.0.2
  - @pnpm/config@20.4.1
  - @pnpm/client@10.0.44
  - @pnpm/package-store@19.0.14
  - @pnpm/server@17.0.6

## 7.0.23

### Patch Changes

- @pnpm/package-store@19.0.14
- @pnpm/server@17.0.6
- @pnpm/client@10.0.43

## 7.0.22

### Patch Changes

- Updated dependencies [c597f72ec]
  - @pnpm/config@20.4.0

## 7.0.21

### Patch Changes

- Updated dependencies [4e71066dd]
  - @pnpm/config@20.3.0
  - @pnpm/package-store@19.0.13
  - @pnpm/cli-meta@5.0.6
  - @pnpm/server@17.0.6
  - @pnpm/client@10.0.42

## 7.0.20

### Patch Changes

- Updated dependencies [672c559e4]
  - @pnpm/config@20.2.0
  - @pnpm/cli-meta@5.0.5
  - @pnpm/package-store@19.0.12
  - @pnpm/server@17.0.5
  - @pnpm/client@10.0.41

## 7.0.19

### Patch Changes

- @pnpm/package-store@19.0.11
- @pnpm/server@17.0.4
- @pnpm/client@10.0.40

## 7.0.18

### Patch Changes

- @pnpm/client@10.0.39
- @pnpm/package-store@19.0.10
- @pnpm/server@17.0.4

## 7.0.17

### Patch Changes

- @pnpm/client@10.0.38
- @pnpm/package-store@19.0.10
- @pnpm/server@17.0.4

## 7.0.16

### Patch Changes

- @pnpm/client@10.0.37
- @pnpm/package-store@19.0.10
- @pnpm/server@17.0.4

## 7.0.15

### Patch Changes

- @pnpm/package-store@19.0.10
- @pnpm/server@17.0.4
- @pnpm/client@10.0.36

## 7.0.14

### Patch Changes

- Updated dependencies [291607c5a]
  - @pnpm/package-store@19.0.9
  - @pnpm/client@10.0.35
  - @pnpm/server@17.0.4
  - @pnpm/config@20.1.2

## 7.0.13

### Patch Changes

- @pnpm/client@10.0.34
- @pnpm/package-store@19.0.8
- @pnpm/server@17.0.3

## 7.0.12

### Patch Changes

- Updated dependencies [7d65d901a]
  - @pnpm/store-path@8.0.1
  - @pnpm/client@10.0.33
  - @pnpm/package-store@19.0.8
  - @pnpm/server@17.0.3
  - @pnpm/config@20.1.1

## 7.0.11

### Patch Changes

- @pnpm/client@10.0.32
- @pnpm/package-store@19.0.7
- @pnpm/server@17.0.2

## 7.0.10

### Patch Changes

- Updated dependencies [43ce9e4a6]
- Updated dependencies [d6592964f]
  - @pnpm/config@20.1.0
  - @pnpm/package-store@19.0.7
  - @pnpm/server@17.0.2
  - @pnpm/cli-meta@5.0.4
  - @pnpm/client@10.0.31

## 7.0.9

### Patch Changes

- @pnpm/client@10.0.30
- @pnpm/package-store@19.0.6
- @pnpm/server@17.0.1

## 7.0.8

### Patch Changes

- @pnpm/package-store@19.0.6
- @pnpm/server@17.0.1
- @pnpm/client@10.0.29

## 7.0.7

### Patch Changes

- Updated dependencies [01bc58e2c]
- Updated dependencies [ac5abd3ff]
- Updated dependencies [b60bb6cbe]
  - @pnpm/package-store@19.0.5
  - @pnpm/config@20.0.0
  - @pnpm/server@17.0.1
  - @pnpm/client@10.0.28

## 7.0.6

### Patch Changes

- @pnpm/package-store@19.0.4
- @pnpm/server@17.0.1
- @pnpm/client@10.0.27

## 7.0.5

### Patch Changes

- @pnpm/package-store@19.0.3
- @pnpm/server@17.0.1
- @pnpm/client@10.0.26

## 7.0.4

### Patch Changes

- Updated dependencies [b1dd0ee58]
  - @pnpm/config@19.2.1

## 7.0.3

### Patch Changes

- Updated dependencies [d774a3196]
- Updated dependencies [832e28826]
  - @pnpm/config@19.2.0
  - @pnpm/cli-meta@5.0.3
  - @pnpm/package-store@19.0.2
  - @pnpm/server@17.0.1
  - @pnpm/client@10.0.25

## 7.0.2

### Patch Changes

- Updated dependencies [ee328fd25]
  - @pnpm/config@19.1.0
  - @pnpm/package-store@19.0.1
  - @pnpm/server@17.0.0

## 7.0.1

### Patch Changes

- @pnpm/client@10.0.24
- @pnpm/package-store@19.0.0
- @pnpm/server@17.0.0

## 7.0.0

### Major Changes

- 9caa33d53: Remove `disableRelinkFromStore` and `relinkLocalDirDeps`. Replace them with `disableRelinkLocalDirDeps`.

### Patch Changes

- Updated dependencies [9caa33d53]
  - @pnpm/server@17.0.0
  - @pnpm/package-store@19.0.0
  - @pnpm/client@10.0.23
  - @pnpm/config@19.0.3

## 6.2.1

### Patch Changes

- @pnpm/package-store@18.0.1
- @pnpm/server@16.0.2
- @pnpm/client@10.0.22

## 6.2.0

### Minor Changes

- 03cdccc6e: New option added: disableRelinkFromStore.

### Patch Changes

- @pnpm/package-store@18.0.0
- @pnpm/server@16.0.2
- @pnpm/config@19.0.2
- @pnpm/client@10.0.21

## 6.1.3

### Patch Changes

- @pnpm/package-store@17.0.2
- @pnpm/server@16.0.1
- @pnpm/client@10.0.20
- @pnpm/config@19.0.1

## 6.1.2

### Patch Changes

- Updated dependencies [548768e09]
  - @pnpm/server@16.0.1
  - @pnpm/package-store@17.0.1
  - @pnpm/client@10.0.19
  - @pnpm/config@19.0.1

## 6.1.1

### Patch Changes

- Updated dependencies [cb8bcc8df]
- Updated dependencies [494f87544]
  - @pnpm/config@19.0.0
  - @pnpm/package-store@17.0.0
  - @pnpm/server@16.0.0
  - @pnpm/client@10.0.18

## 6.1.0

### Minor Changes

- 92f42224c: New option added: `relinkLocalDirDeps`. It is `true` by default. When `false`, local directory dependencies are not relinked on repeat install.

### Patch Changes

- Updated dependencies [92f42224c]
  - @pnpm/package-store@16.1.0
  - @pnpm/client@10.0.17
  - @pnpm/server@15.0.3

## 6.0.24

### Patch Changes

- @pnpm/client@10.0.16
- @pnpm/package-store@16.0.12
- @pnpm/server@15.0.3

## 6.0.23

### Patch Changes

- @pnpm/package-store@16.0.12
- @pnpm/server@15.0.3

## 6.0.22

### Patch Changes

- @pnpm/package-store@16.0.11
- @pnpm/server@15.0.3
- @pnpm/config@18.4.4

## 6.0.21

### Patch Changes

- @pnpm/package-store@16.0.10
- @pnpm/server@15.0.3
- @pnpm/client@10.0.15
- @pnpm/config@18.4.4

## 6.0.20

### Patch Changes

- @pnpm/package-store@16.0.9
- @pnpm/server@15.0.3
- @pnpm/config@18.4.4

## 6.0.19

### Patch Changes

- @pnpm/client@10.0.14
- @pnpm/package-store@16.0.8
- @pnpm/server@15.0.3

## 6.0.18

### Patch Changes

- @pnpm/cli-meta@5.0.2
- @pnpm/config@18.4.4
- @pnpm/package-store@16.0.8
- @pnpm/server@15.0.3
- @pnpm/client@10.0.13

## 6.0.17

### Patch Changes

- @pnpm/config@18.4.3
- @pnpm/client@10.0.12
- @pnpm/package-store@16.0.7
- @pnpm/server@15.0.2

## 6.0.16

### Patch Changes

- @pnpm/package-store@16.0.7
- @pnpm/server@15.0.2
- @pnpm/client@10.0.11
- @pnpm/config@18.4.2

## 6.0.15

### Patch Changes

- @pnpm/client@10.0.10
- @pnpm/package-store@16.0.6
- @pnpm/server@15.0.2

## 6.0.14

### Patch Changes

- @pnpm/client@10.0.9
- @pnpm/package-store@16.0.6
- @pnpm/server@15.0.2

## 6.0.13

### Patch Changes

- Updated dependencies [e2d631217]
  - @pnpm/config@18.4.2
  - @pnpm/package-store@16.0.6
  - @pnpm/server@15.0.2

## 6.0.12

### Patch Changes

- @pnpm/config@18.4.1
- @pnpm/error@5.0.2
- @pnpm/package-store@16.0.5
- @pnpm/client@10.0.8
- @pnpm/server@15.0.2

## 6.0.11

### Patch Changes

- Updated dependencies [4b97f1f07]
- Updated dependencies [d55b41a8b]
  - @pnpm/package-store@16.0.4
  - @pnpm/server@15.0.2
  - @pnpm/client@10.0.7
  - @pnpm/config@18.4.0

## 6.0.10

### Patch Changes

- Updated dependencies [301b8e2da]
  - @pnpm/config@18.4.0
  - @pnpm/cli-meta@5.0.1
  - @pnpm/package-store@16.0.3
  - @pnpm/server@15.0.2
  - @pnpm/error@5.0.1
  - @pnpm/client@10.0.6

## 6.0.9

### Patch Changes

- Updated dependencies [1de07a4af]
  - @pnpm/config@18.3.2

## 6.0.8

### Patch Changes

- Updated dependencies [2809e89ab]
  - @pnpm/config@18.3.1
  - @pnpm/client@10.0.5
  - @pnpm/package-store@16.0.2
  - @pnpm/server@15.0.1

## 6.0.7

### Patch Changes

- @pnpm/client@10.0.4
- @pnpm/server@15.0.1
- @pnpm/package-store@16.0.2

## 6.0.6

### Patch Changes

- Updated dependencies [32f8e08c6]
  - @pnpm/config@18.3.0
  - @pnpm/package-store@16.0.2
  - @pnpm/server@15.0.0
  - @pnpm/client@10.0.3

## 6.0.5

### Patch Changes

- Updated dependencies [fc8780ca9]
  - @pnpm/config@18.2.0

## 6.0.4

### Patch Changes

- @pnpm/config@18.1.1
- @pnpm/package-store@16.0.1
- @pnpm/server@15.0.0
- @pnpm/client@10.0.2

## 6.0.3

### Patch Changes

- Updated dependencies [e2cb4b63d]
- Updated dependencies [cd6ce11f0]
  - @pnpm/config@18.1.0
  - @pnpm/client@10.0.1
  - @pnpm/package-store@16.0.0
  - @pnpm/server@15.0.0

## 6.0.2

### Patch Changes

- @pnpm/config@18.0.2

## 6.0.1

### Patch Changes

- @pnpm/config@18.0.1

## 6.0.0

### Major Changes

- 7a0ce1df0: When there's a `files` field in the `package.json`, only deploy those files that are listed in it.
  Use the same logic also when injecting packages. This behavior can be changed by setting the `deploy-all-files` setting to `true` [#5911](https://github.com/pnpm/pnpm/issues/5911).
- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [7a0ce1df0]
- Updated dependencies [158d8cf22]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [8e35c21d1]
- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [113f0ae26]
  - @pnpm/config@18.0.0
  - @pnpm/client@10.0.0
  - @pnpm/package-store@16.0.0
  - @pnpm/store-path@8.0.0
  - @pnpm/error@5.0.0
  - @pnpm/cli-meta@5.0.0
  - @pnpm/server@15.0.0

## 5.2.20

### Patch Changes

- @pnpm/config@17.0.2

## 5.2.19

### Patch Changes

- Updated dependencies [b38d711f3]
  - @pnpm/config@17.0.1

## 5.2.18

### Patch Changes

- Updated dependencies [e505b58e3]
  - @pnpm/config@17.0.0
  - @pnpm/client@9.1.5
  - @pnpm/package-store@15.1.8
  - @pnpm/server@14.1.2

## 5.2.17

### Patch Changes

- @pnpm/config@16.7.2

## 5.2.16

### Patch Changes

- @pnpm/config@16.7.1

## 5.2.15

### Patch Changes

- Updated dependencies [5c31fa8be]
  - @pnpm/config@16.7.0

## 5.2.14

### Patch Changes

- @pnpm/config@16.6.4

## 5.2.13

### Patch Changes

- @pnpm/config@16.6.3

## 5.2.12

### Patch Changes

- @pnpm/client@9.1.4
- @pnpm/server@14.1.2
- @pnpm/package-store@15.1.7
- @pnpm/config@16.6.2

## 5.2.11

### Patch Changes

- @pnpm/client@9.1.3
- @pnpm/package-store@15.1.7
- @pnpm/config@16.6.1
- @pnpm/server@14.1.1

## 5.2.10

### Patch Changes

- Updated dependencies [59ee53678]
  - @pnpm/config@16.6.0
  - @pnpm/package-store@15.1.6
  - @pnpm/server@14.1.0
  - @pnpm/client@9.1.2

## 5.2.9

### Patch Changes

- @pnpm/package-store@15.1.5
- @pnpm/server@14.1.0
- @pnpm/config@16.5.5

## 5.2.8

### Patch Changes

- @pnpm/package-store@15.1.4
- @pnpm/server@14.1.0
- @pnpm/config@16.5.4

## 5.2.7

### Patch Changes

- @pnpm/config@16.5.3

## 5.2.6

### Patch Changes

- @pnpm/config@16.5.2

## 5.2.5

### Patch Changes

- @pnpm/package-store@15.1.3
- @pnpm/server@14.1.0
- @pnpm/config@16.5.1

## 5.2.4

### Patch Changes

- Updated dependencies [28b47a156]
  - @pnpm/config@16.5.0

## 5.2.3

### Patch Changes

- Updated dependencies [1e6de89b6]
  - @pnpm/package-store@15.1.2
  - @pnpm/server@14.1.0
  - @pnpm/client@9.1.1
  - @pnpm/config@16.4.3

## 5.2.2

### Patch Changes

- @pnpm/config@16.4.2

## 5.2.1

### Patch Changes

- @pnpm/package-store@15.1.1
- @pnpm/server@14.1.0
- @pnpm/config@16.4.1

## 5.2.0

### Minor Changes

- c7b05cd9a: When ignoreScripts=true is passed to the fetcher, do not build git-hosted dependencies.

### Patch Changes

- Updated dependencies [891a8d763]
- Updated dependencies [c7b05cd9a]
- Updated dependencies [3ebce5db7]
  - @pnpm/package-store@15.1.0
  - @pnpm/server@14.1.0
  - @pnpm/client@9.1.0
  - @pnpm/config@16.4.0
  - @pnpm/error@4.0.1

## 5.1.14

### Patch Changes

- Updated dependencies [1fad508b0]
  - @pnpm/config@16.3.0

## 5.1.13

### Patch Changes

- ec97a3105: Report to the console when a git-hosted dependency is built [#5847](https://github.com/pnpm/pnpm/pull/5847).
- Updated dependencies [ec97a3105]
  - @pnpm/client@9.0.1
  - @pnpm/package-store@15.0.5
  - @pnpm/server@14.0.5
  - @pnpm/config@16.2.2

## 5.1.12

### Patch Changes

- Updated dependencies [d71dbf230]
  - @pnpm/config@16.2.1

## 5.1.11

### Patch Changes

- Updated dependencies [339c0a704]
- Updated dependencies [841f52e70]
  - @pnpm/client@9.0.0
  - @pnpm/config@16.2.0
  - @pnpm/package-store@15.0.5
  - @pnpm/server@14.0.5

## 5.1.10

### Patch Changes

- @pnpm/cli-meta@4.0.3
- @pnpm/config@16.1.11
- @pnpm/package-store@15.0.5
- @pnpm/server@14.0.5
- @pnpm/client@8.1.3

## 5.1.9

### Patch Changes

- @pnpm/config@16.1.10
- @pnpm/package-store@15.0.4
- @pnpm/server@14.0.4

## 5.1.8

### Patch Changes

- @pnpm/config@16.1.9

## 5.1.7

### Patch Changes

- @pnpm/config@16.1.8

## 5.1.6

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/config@16.1.7
  - @pnpm/package-store@15.0.3
  - @pnpm/client@8.1.2
  - @pnpm/server@14.0.4

## 5.1.5

### Patch Changes

- @pnpm/config@16.1.6

## 5.1.4

### Patch Changes

- @pnpm/config@16.1.5

## 5.1.3

### Patch Changes

- @pnpm/config@16.1.4
- @pnpm/client@8.1.1
- @pnpm/package-store@15.0.2
- @pnpm/server@14.0.3

## 5.1.2

### Patch Changes

- @pnpm/config@16.1.3

## 5.1.1

### Patch Changes

- @pnpm/config@16.1.2

## 5.1.0

### Minor Changes

- eacff33e4: New option added to resolve symlinks to their real locations, when injecting directories.

### Patch Changes

- Updated dependencies [eacff33e4]
  - @pnpm/client@8.1.0
  - @pnpm/package-store@15.0.2
  - @pnpm/server@14.0.3
  - @pnpm/config@16.1.1

## 5.0.6

### Patch Changes

- Updated dependencies [3dab7f83c]
  - @pnpm/config@16.1.0

## 5.0.5

### Patch Changes

- @pnpm/client@8.0.3
- @pnpm/cli-meta@4.0.2
- @pnpm/config@16.0.5
- @pnpm/package-store@15.0.2
- @pnpm/server@14.0.3

## 5.0.4

### Patch Changes

- @pnpm/config@16.0.4

## 5.0.3

### Patch Changes

- Updated dependencies [aacb83f73]
- Updated dependencies [a14ad09e6]
  - @pnpm/config@16.0.3

## 5.0.2

### Patch Changes

- Updated dependencies [bea0acdfc]
  - @pnpm/config@16.0.2
  - @pnpm/client@8.0.2
  - @pnpm/package-store@15.0.1
  - @pnpm/server@14.0.2

## 5.0.1

### Patch Changes

- Updated dependencies [e7fd8a84c]
  - @pnpm/config@16.0.1
  - @pnpm/cli-meta@4.0.1
  - @pnpm/package-store@15.0.1
  - @pnpm/server@14.0.1
  - @pnpm/client@8.0.1

## 5.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [1d0fd82fd]
- Updated dependencies [645384bfd]
- Updated dependencies [f884689e0]
- Updated dependencies [3c117996e]
  - @pnpm/cli-meta@4.0.0
  - @pnpm/client@8.0.0
  - @pnpm/config@16.0.0
  - @pnpm/error@4.0.0
  - @pnpm/package-store@15.0.0
  - @pnpm/server@14.0.0
  - @pnpm/store-path@7.0.0

## 4.3.16

### Patch Changes

- Updated dependencies [147ec6eaf]
  - @pnpm/server@13.0.9
  - @pnpm/config@15.10.12
  - @pnpm/client@7.2.10
  - @pnpm/package-store@14.2.7

## 4.3.15

### Patch Changes

- @pnpm/client@7.2.9
- @pnpm/server@13.0.8
- @pnpm/package-store@14.2.7
- @pnpm/config@15.10.11

## 4.3.14

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/config@15.10.10
  - @pnpm/client@7.2.8
  - @pnpm/package-store@14.2.6
  - @pnpm/server@13.0.7

## 4.3.13

### Patch Changes

- @pnpm/config@15.10.9
- @pnpm/cli-meta@3.0.8
- @pnpm/package-store@14.2.5
- @pnpm/server@13.0.7
- @pnpm/client@7.2.7

## 4.3.12

### Patch Changes

- @pnpm/config@15.10.8

## 4.3.11

### Patch Changes

- @pnpm/config@15.10.7

## 4.3.10

### Patch Changes

- @pnpm/cli-meta@3.0.7
- @pnpm/config@15.10.6
- @pnpm/package-store@14.2.4
- @pnpm/server@13.0.6
- @pnpm/client@7.2.6

## 4.3.9

### Patch Changes

- @pnpm/client@7.2.5
- @pnpm/package-store@14.2.3
- @pnpm/server@13.0.5
- @pnpm/config@15.10.5

## 4.3.8

### Patch Changes

- @pnpm/config@15.10.4

## 4.3.7

### Patch Changes

- @pnpm/client@7.2.4
- @pnpm/package-store@14.2.3
- @pnpm/server@13.0.5
- @pnpm/config@15.10.3

## 4.3.6

### Patch Changes

- @pnpm/client@7.2.3
- @pnpm/package-store@14.2.3
- @pnpm/server@13.0.5
- @pnpm/config@15.10.2

## 4.3.5

### Patch Changes

- 17e69e18b: `pnpm store prune` should remove all cached metadata.
- Updated dependencies [17e69e18b]
  - @pnpm/package-store@14.2.3
  - @pnpm/server@13.0.5
  - @pnpm/client@7.2.2
  - @pnpm/config@15.10.1

## 4.3.4

### Patch Changes

- Updated dependencies [2aa22e4b1]
  - @pnpm/config@15.10.0

## 4.3.3

### Patch Changes

- @pnpm/config@15.9.4

## 4.3.2

### Patch Changes

- @pnpm/package-store@14.2.2
- @pnpm/server@13.0.5
- @pnpm/config@15.9.3

## 4.3.1

### Patch Changes

- Updated dependencies [dbac0ca01]
  - @pnpm/package-store@14.2.1
  - @pnpm/server@13.0.5
  - @pnpm/client@7.2.1
  - @pnpm/config@15.9.2

## 4.3.0

### Minor Changes

- 23984abd1: Add hook for adding custom fetchers.

### Patch Changes

- Updated dependencies [23984abd1]
  - @pnpm/client@7.2.0
  - @pnpm/package-store@14.2.0
  - @pnpm/server@13.0.5
  - @pnpm/config@15.9.1

## 4.2.1

### Patch Changes

- @pnpm/package-store@14.1.1
- @pnpm/server@13.0.4
- @pnpm/client@7.1.14
- @pnpm/config@15.9.0

## 4.2.0

### Minor Changes

- 65c4260de: Support a new hook for passing a custom package importer to the store controller.

### Patch Changes

- Updated dependencies [39c040127]
- Updated dependencies [43cd6aaca]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
- Updated dependencies [29a81598a]
  - @pnpm/server@13.0.4
  - @pnpm/config@15.9.0
  - @pnpm/package-store@14.1.0
  - @pnpm/client@7.1.13

## 4.1.26

### Patch Changes

- Updated dependencies [34121d753]
  - @pnpm/config@15.8.1
  - @pnpm/cli-meta@3.0.6
  - @pnpm/package-store@14.0.7
  - @pnpm/server@13.0.3
  - @pnpm/client@7.1.12

## 4.1.25

### Patch Changes

- Updated dependencies [cac34ad69]
- Updated dependencies [99019e071]
  - @pnpm/config@15.8.0
  - @pnpm/package-store@14.0.6
  - @pnpm/server@13.0.2

## 4.1.24

### Patch Changes

- @pnpm/config@15.7.1
- @pnpm/package-store@14.0.5

## 4.1.23

### Patch Changes

- Updated dependencies [4fa1091c8]
  - @pnpm/config@15.7.0
  - @pnpm/client@7.1.11
  - @pnpm/package-store@14.0.5
  - @pnpm/server@13.0.2

## 4.1.22

### Patch Changes

- Updated dependencies [7334b347b]
  - @pnpm/config@15.6.1

## 4.1.21

### Patch Changes

- Updated dependencies [28f000509]
- Updated dependencies [406656f80]
  - @pnpm/config@15.6.0
  - @pnpm/client@7.1.10
  - @pnpm/server@13.0.2
  - @pnpm/package-store@14.0.5

## 4.1.20

### Patch Changes

- @pnpm/config@15.5.2

## 4.1.19

### Patch Changes

- @pnpm/package-store@14.0.5
- @pnpm/server@13.0.1

## 4.1.18

### Patch Changes

- @pnpm/client@7.1.9
- @pnpm/package-store@14.0.4
- @pnpm/server@13.0.1

## 4.1.17

### Patch Changes

- Updated dependencies [5f643f23b]
  - @pnpm/config@15.5.1
  - @pnpm/package-store@14.0.4
  - @pnpm/client@7.1.8
  - @pnpm/server@13.0.1

## 4.1.16

### Patch Changes

- @pnpm/package-store@14.0.3
- @pnpm/server@13.0.1

## 4.1.15

### Patch Changes

- @pnpm/package-store@14.0.2
- @pnpm/server@13.0.1

## 4.1.14

### Patch Changes

- Updated dependencies [f48d46ef6]
  - @pnpm/config@15.5.0

## 4.1.13

### Patch Changes

- @pnpm/cli-meta@3.0.5
- @pnpm/config@15.4.1
- @pnpm/package-store@14.0.1
- @pnpm/server@13.0.1
- @pnpm/client@7.1.7

## 4.1.12

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/package-store@14.0.0
  - @pnpm/server@13.0.0
  - @pnpm/config@15.4.0
  - @pnpm/cli-meta@3.0.4
  - @pnpm/client@7.1.6

## 4.1.11

### Patch Changes

- Updated dependencies [56cf04cb3]
  - @pnpm/config@15.3.0
  - @pnpm/cli-meta@3.0.3
  - @pnpm/package-store@13.0.8
  - @pnpm/server@12.0.5
  - @pnpm/client@7.1.5

## 4.1.10

### Patch Changes

- Updated dependencies [8c8156165]
- Updated dependencies [25798aad1]
  - @pnpm/server@12.0.4
  - @pnpm/config@15.2.1

## 4.1.9

### Patch Changes

- Updated dependencies [bc80631d3]
- Updated dependencies [d5730ba81]
  - @pnpm/config@15.2.0
  - @pnpm/cli-meta@3.0.2
  - @pnpm/package-store@13.0.7
  - @pnpm/server@12.0.3
  - @pnpm/client@7.1.4

## 4.1.8

### Patch Changes

- @pnpm/package-store@13.0.6
- @pnpm/server@12.0.2
- @pnpm/client@7.1.3
- @pnpm/config@15.1.4

## 4.1.7

### Patch Changes

- Updated dependencies [ae2f845c5]
  - @pnpm/config@15.1.4

## 4.1.6

### Patch Changes

- Updated dependencies [05159665d]
  - @pnpm/config@15.1.3

## 4.1.5

### Patch Changes

- Updated dependencies [af22c6c4f]
  - @pnpm/config@15.1.2
  - @pnpm/package-store@13.0.5
  - @pnpm/server@12.0.1
  - @pnpm/client@7.1.2

## 4.1.4

### Patch Changes

- @pnpm/package-store@13.0.4
- @pnpm/server@12.0.1

## 4.1.3

### Patch Changes

- @pnpm/package-store@13.0.3
- @pnpm/server@12.0.1
- @pnpm/config@15.1.1

## 4.1.2

### Patch Changes

- @pnpm/package-store@13.0.2
- @pnpm/cli-meta@3.0.1
- @pnpm/config@15.1.1
- @pnpm/server@12.0.1
- @pnpm/client@7.1.1

## 4.1.1

### Patch Changes

- Updated dependencies [e05dcc48a]
  - @pnpm/config@15.1.0

## 4.1.0

### Minor Changes

- c6463b9fd: New setting added: `git-shallow-hosts`. When cloning repositories from "shallow-hosts", pnpm will use shallow cloning to fetch only the needed commit, not all the history [#4548](https://github.com/pnpm/pnpm/pull/4548).

### Patch Changes

- Updated dependencies [cdeb65203]
- Updated dependencies [8dac029ef]
- Updated dependencies [72b79f55a]
- Updated dependencies [546e644e9]
- Updated dependencies [c6463b9fd]
- Updated dependencies [4bed585e2]
- Updated dependencies [8fa95fd86]
  - @pnpm/store-path@6.0.0
  - @pnpm/config@15.0.0
  - @pnpm/client@7.1.0
  - @pnpm/package-store@13.0.1
  - @pnpm/server@12.0.0
  - @pnpm/error@3.0.1

## 4.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [516859178]
- Updated dependencies [73d71a2d5]
- Updated dependencies [fa656992c]
- Updated dependencies [542014839]
- Updated dependencies [585e9ca9e]
  - @pnpm/config@14.0.0
  - @pnpm/cli-meta@3.0.0
  - @pnpm/client@7.0.0
  - @pnpm/error@3.0.0
  - @pnpm/package-store@13.0.0
  - @pnpm/server@12.0.0

## 3.2.10

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/config@13.13.2
  - @pnpm/package-store@12.1.12
  - @pnpm/server@11.0.19
  - @pnpm/client@6.1.3

## 3.2.9

### Patch Changes

- @pnpm/package-store@12.1.11
- @pnpm/server@11.0.18
- @pnpm/cli-meta@2.0.2
- @pnpm/config@13.13.1
- @pnpm/client@6.1.2

## 3.2.8

### Patch Changes

- Updated dependencies [fa4f9133b]
  - @pnpm/package-store@12.1.10
  - @pnpm/server@11.0.17

## 3.2.7

### Patch Changes

- Updated dependencies [50e347d23]
  - @pnpm/package-store@12.1.9
  - @pnpm/server@11.0.17

## 3.2.6

### Patch Changes

- Updated dependencies [334e5340a]
  - @pnpm/config@13.13.0

## 3.2.5

### Patch Changes

- Updated dependencies [b7566b979]
  - @pnpm/config@13.12.0

## 3.2.4

### Patch Changes

- Updated dependencies [fff0e4493]
  - @pnpm/config@13.11.0

## 3.2.3

### Patch Changes

- Updated dependencies [e76151f66]
  - @pnpm/config@13.10.0
  - @pnpm/client@6.1.1
  - @pnpm/cli-meta@2.0.1
  - @pnpm/package-store@12.1.8
  - @pnpm/server@11.0.17

## 3.2.2

### Patch Changes

- @pnpm/package-store@12.1.7
- @pnpm/server@11.0.16

## 3.2.1

### Patch Changes

- Updated dependencies [8fe8f5e55]
  - @pnpm/config@13.9.0
  - @pnpm/package-store@12.1.6

## 3.2.0

### Minor Changes

- a6cf11cb7: New optional setting added: userConfig. userConfig may contain token helpers.

### Patch Changes

- Updated dependencies [a6cf11cb7]
- Updated dependencies [732d4962f]
- Updated dependencies [a6cf11cb7]
  - @pnpm/client@6.1.0
  - @pnpm/config@13.8.0
  - @pnpm/package-store@12.1.6
  - @pnpm/server@11.0.16

## 3.1.17

### Patch Changes

- @pnpm/cli-meta@2.0.0
- @pnpm/config@13.7.2
- @pnpm/package-store@12.1.6
- @pnpm/server@11.0.16
- @pnpm/client@6.0.11

## 3.1.16

### Patch Changes

- @pnpm/cli-meta@2.0.0
- @pnpm/config@13.7.1
- @pnpm/package-store@12.1.5
- @pnpm/server@11.0.15
- @pnpm/client@6.0.10

## 3.1.15

### Patch Changes

- Updated dependencies [30bfca967]
- Updated dependencies [927c4a089]
- Updated dependencies [10a4bd4db]
- Updated dependencies [d00e1fc6a]
  - @pnpm/config@13.7.0
  - @pnpm/package-store@12.1.4
  - @pnpm/server@11.0.14
  - @pnpm/cli-meta@2.0.0
  - @pnpm/client@6.0.9

## 3.1.14

### Patch Changes

- @pnpm/client@6.0.8
- @pnpm/package-store@12.1.3
- @pnpm/server@11.0.13

## 3.1.13

### Patch Changes

- Updated dependencies [46aaf7108]
  - @pnpm/config@13.6.1
  - @pnpm/client@6.0.7
  - @pnpm/server@11.0.13
  - @pnpm/package-store@12.1.3

## 3.1.12

### Patch Changes

- Updated dependencies [8a99a01ff]
  - @pnpm/config@13.6.0

## 3.1.11

### Patch Changes

- @pnpm/client@6.0.6
- @pnpm/package-store@12.1.3
- @pnpm/server@11.0.12

## 3.1.10

### Patch Changes

- Updated dependencies [a7ff2d5ce]
  - @pnpm/config@13.5.1
  - @pnpm/package-store@12.1.3
  - @pnpm/server@11.0.12

## 3.1.9

### Patch Changes

- Updated dependencies [002778559]
  - @pnpm/config@13.5.0
  - @pnpm/client@6.0.5
  - @pnpm/server@11.0.12
  - @pnpm/package-store@12.1.2

## 3.1.8

### Patch Changes

- @pnpm/client@6.0.4
- @pnpm/package-store@12.1.2
- @pnpm/server@11.0.11

## 3.1.7

### Patch Changes

- @pnpm/client@6.0.3
- @pnpm/package-store@12.1.2
- @pnpm/server@11.0.11

## 3.1.6

### Patch Changes

- 1647d8e2f: When the store location is a relative location, pick the store location relative to the workspace root directory location [#3976](https://github.com/pnpm/pnpm/issues/3976).
  - @pnpm/package-store@12.1.2
  - @pnpm/server@11.0.11

## 3.1.5

### Patch Changes

- @pnpm/client@6.0.2
- @pnpm/config@13.4.2
- @pnpm/cli-meta@2.0.0
- @pnpm/package-store@12.1.1
- @pnpm/server@11.0.11

## 3.1.4

### Patch Changes

- @pnpm/client@6.0.1
- @pnpm/package-store@12.1.0
- @pnpm/server@11.0.10

## 3.1.3

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/package-store@12.1.0
  - @pnpm/client@6.0.0
  - @pnpm/cli-meta@2.0.0
  - @pnpm/config@13.4.1
  - @pnpm/server@11.0.10

## 3.1.2

### Patch Changes

- @pnpm/client@5.0.10
- @pnpm/server@11.0.9
- @pnpm/package-store@12.0.15

## 3.1.1

### Patch Changes

- Updated dependencies [b6d74c545]
  - @pnpm/config@13.4.0
  - @pnpm/client@5.0.9
  - @pnpm/package-store@12.0.15
  - @pnpm/server@11.0.8

## 3.1.0

### Minor Changes

- bd7bcdbe8: Make the maximum amount of sockets configurable through the `maxSockets` option.

### Patch Changes

- Updated dependencies [bd7bcdbe8]
  - @pnpm/config@13.3.0
  - @pnpm/client@5.0.8
  - @pnpm/server@11.0.8
  - @pnpm/package-store@12.0.15

## 3.0.20

### Patch Changes

- Updated dependencies [5ee3b2dc7]
  - @pnpm/config@13.2.0

## 3.0.19

### Patch Changes

- Updated dependencies [4027a3c69]
  - @pnpm/config@13.1.0

## 3.0.18

### Patch Changes

- @pnpm/client@5.0.7
- @pnpm/package-store@12.0.15
- @pnpm/server@11.0.7

## 3.0.17

### Patch Changes

- Updated dependencies [fe5688dc0]
- Updated dependencies [c7081cbb4]
- Updated dependencies [c7081cbb4]
  - @pnpm/config@13.0.0

## 3.0.16

### Patch Changes

- Updated dependencies [d62259d67]
  - @pnpm/config@12.6.0

## 3.0.15

### Patch Changes

- @pnpm/client@5.0.6
- @pnpm/package-store@12.0.15
- @pnpm/server@11.0.7

## 3.0.14

### Patch Changes

- Updated dependencies [6681fdcbc]
- Updated dependencies [bab172385]
  - @pnpm/config@12.5.0
  - @pnpm/server@11.0.7
  - @pnpm/package-store@12.0.15
  - @pnpm/client@5.0.5

## 3.0.13

### Patch Changes

- @pnpm/client@5.0.4
- @pnpm/server@11.0.6
- @pnpm/package-store@12.0.14

## 3.0.12

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9

## 3.0.11

### Patch Changes

- @pnpm/config@12.4.8

## 3.0.10

### Patch Changes

- @pnpm/client@5.0.3
- @pnpm/package-store@12.0.14
- @pnpm/server@11.0.5

## 3.0.9

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7
  - @pnpm/package-store@12.0.14
  - @pnpm/server@11.0.5

## 3.0.8

### Patch Changes

- @pnpm/package-store@12.0.13
- @pnpm/server@11.0.5

## 3.0.7

### Patch Changes

- @pnpm/client@5.0.2
- @pnpm/package-store@12.0.12
- @pnpm/server@11.0.5

## 3.0.6

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6

## 3.0.5

### Patch Changes

- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5

## 3.0.4

### Patch Changes

- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4

## 3.0.3

### Patch Changes

- @pnpm/cli-meta@2.0.0
- @pnpm/config@12.4.3
- @pnpm/package-store@12.0.12
- @pnpm/server@11.0.5
- @pnpm/client@5.0.1

## 3.0.2

### Patch Changes

- Updated dependencies [73c1f802e]
  - @pnpm/config@12.4.2

## 3.0.1

### Patch Changes

- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1

## 3.0.0

### Major Changes

- 691f64713: New required option added: cacheDir.

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [691f64713]
- Updated dependencies [691f64713]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0
  - @pnpm/client@5.0.0
  - @pnpm/package-store@12.0.11
  - @pnpm/server@11.0.4

## 2.1.11

### Patch Changes

- @pnpm/cli-meta@2.0.0
- @pnpm/config@12.3.3
- @pnpm/package-store@12.0.11
- @pnpm/server@11.0.4
- @pnpm/client@4.0.2

## 2.1.10

### Patch Changes

- @pnpm/package-store@12.0.10
- @pnpm/server@11.0.3

## 2.1.9

### Patch Changes

- @pnpm/client@4.0.1
- @pnpm/package-store@12.0.9
- @pnpm/server@11.0.3

## 2.1.8

### Patch Changes

- Updated dependencies [eeff424bd]
  - @pnpm/client@4.0.0
  - @pnpm/server@11.0.3
  - @pnpm/package-store@12.0.9
  - @pnpm/cli-meta@2.0.0
  - @pnpm/config@12.3.2

## 2.1.7

### Patch Changes

- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1
  - @pnpm/package-store@12.0.8
  - @pnpm/server@11.0.2
  - @pnpm/client@3.1.6

## 2.1.6

### Patch Changes

- @pnpm/client@3.1.5
- @pnpm/package-store@12.0.7
- @pnpm/server@11.0.2

## 2.1.5

### Patch Changes

- Updated dependencies [84ec82e05]
- Updated dependencies [c2a71e4fd]
- Updated dependencies [84ec82e05]
  - @pnpm/config@12.3.0

## 2.1.4

### Patch Changes

- @pnpm/package-store@12.0.7
- @pnpm/server@11.0.2
- @pnpm/client@3.1.4

## 2.1.3

### Patch Changes

- @pnpm/package-store@12.0.6
- @pnpm/server@11.0.2
- @pnpm/client@3.1.3
- @pnpm/config@12.2.0

## 2.1.2

### Patch Changes

- Updated dependencies [3b147ced9]
  - @pnpm/package-store@12.0.5
  - @pnpm/client@3.1.2
  - @pnpm/server@11.0.2

## 2.1.1

### Patch Changes

- @pnpm/client@3.1.1
- @pnpm/package-store@12.0.4
- @pnpm/server@11.0.2
- @pnpm/config@12.2.0

## 2.1.0

### Minor Changes

- 05baaa6e7: Add new config setting: `fetch-timeout`.

### Patch Changes

- Updated dependencies [05baaa6e7]
- Updated dependencies [dfdf669e6]
- Updated dependencies [05baaa6e7]
  - @pnpm/config@12.2.0
  - @pnpm/client@3.1.0
  - @pnpm/server@11.0.1
  - @pnpm/package-store@12.0.3
  - @pnpm/cli-meta@2.0.0

## 2.0.3

### Patch Changes

- Updated dependencies [ba5231ccf]
  - @pnpm/config@12.1.0

## 2.0.2

### Patch Changes

- Updated dependencies [6f198457d]
- Updated dependencies [e3d9b3215]
  - @pnpm/package-store@12.0.2
  - @pnpm/server@11.0.0
  - @pnpm/client@3.0.1
  - @pnpm/config@12.0.0

## 2.0.1

### Patch Changes

- @pnpm/package-store@12.0.1
- @pnpm/server@11.0.0

## 2.0.0

### Minor Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 7adc6e875: Update dependencies.
- Updated dependencies [97b986fbc]
- Updated dependencies [78470a32d]
- Updated dependencies [aed712455]
- Updated dependencies [83645c8ed]
- Updated dependencies [aed712455]
  - @pnpm/cli-meta@2.0.0
  - @pnpm/client@3.0.0
  - @pnpm/config@12.0.0
  - @pnpm/error@2.0.0
  - @pnpm/package-store@12.0.0
  - @pnpm/server@11.0.0

## 1.0.4

### Patch Changes

- Updated dependencies [4f1ce907a]
  - @pnpm/config@11.14.2

## 1.0.3

### Patch Changes

- Updated dependencies [4b3852c39]
  - @pnpm/config@11.14.1
  - @pnpm/package-store@11.0.3
  - @pnpm/server@10.0.1

## 1.0.2

### Patch Changes

- @pnpm/client@2.0.24
- @pnpm/server@10.0.1
- @pnpm/package-store@11.0.2

## 1.0.1

### Patch Changes

- Updated dependencies [632352f26]
  - @pnpm/package-store@11.0.1
  - @pnpm/server@10.0.0

## 1.0.0

### Major Changes

- 8d1dfa89c: Breaking changes to the store controller API.

  The options to `requestPackage()` and `fetchPackage()` changed.

### Patch Changes

- Updated dependencies [8d1dfa89c]
  - @pnpm/package-store@11.0.0
  - @pnpm/server@10.0.0
  - @pnpm/client@2.0.23
  - @pnpm/config@11.14.0

## 0.3.64

### Patch Changes

- 27a40321c: Update dependencies.
  - @pnpm/client@2.0.22
  - @pnpm/package-store@10.1.18
  - @pnpm/server@9.0.7

## 0.3.63

### Patch Changes

- Updated dependencies [cb040ae18]
  - @pnpm/config@11.14.0

## 0.3.62

### Patch Changes

- Updated dependencies [c4cc62506]
  - @pnpm/config@11.13.0
  - @pnpm/client@2.0.21
  - @pnpm/package-store@10.1.17
  - @pnpm/server@9.0.7

## 0.3.61

### Patch Changes

- Updated dependencies [bff84dbca]
  - @pnpm/config@11.12.1

## 0.3.60

### Patch Changes

- 43de80034: Don't fail when the code is executed through piping to Node's stdin.
- Updated dependencies [43de80034]
  - @pnpm/cli-meta@1.0.2

## 0.3.59

### Patch Changes

- Updated dependencies [548f28df9]
  - @pnpm/config@11.12.0
  - @pnpm/cli-meta@1.0.1
  - @pnpm/package-store@10.1.16
  - @pnpm/server@9.0.7
  - @pnpm/client@2.0.20

## 0.3.58

### Patch Changes

- @pnpm/config@11.11.1

## 0.3.57

### Patch Changes

- Updated dependencies [f40bc5927]
  - @pnpm/config@11.11.0

## 0.3.56

### Patch Changes

- Updated dependencies [425c7547d]
  - @pnpm/config@11.10.2
  - @pnpm/client@2.0.19
  - @pnpm/package-store@10.1.15
  - @pnpm/server@9.0.6

## 0.3.55

### Patch Changes

- Updated dependencies [ea09da716]
  - @pnpm/config@11.10.1

## 0.3.54

### Patch Changes

- Updated dependencies [a8656b42f]
  - @pnpm/config@11.10.0

## 0.3.53

### Patch Changes

- Updated dependencies [041537bc3]
  - @pnpm/config@11.9.1

## 0.3.52

### Patch Changes

- @pnpm/client@2.0.18
- @pnpm/package-store@10.1.14
- @pnpm/server@9.0.6

## 0.3.51

### Patch Changes

- dc5a0a102: The maximum number of allowed connections increased to 3 times the number of network concurrency. This should fix the socket timeout issues that sometimes happen.
  - @pnpm/client@2.0.17
  - @pnpm/server@9.0.6
  - @pnpm/package-store@10.1.13

## 0.3.50

### Patch Changes

- @pnpm/client@2.0.16
- @pnpm/server@9.0.5
- @pnpm/package-store@10.1.12

## 0.3.49

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0
  - @pnpm/package-store@10.1.11
  - @pnpm/server@9.0.4
  - @pnpm/client@2.0.15

## 0.3.48

### Patch Changes

- Updated dependencies [fcc1c7100]
  - @pnpm/config@11.8.0
  - @pnpm/client@2.0.14
  - @pnpm/package-store@10.1.10
  - @pnpm/server@9.0.3

## 0.3.47

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/client@2.0.13
  - @pnpm/config@11.7.2
  - @pnpm/package-store@10.1.9
  - @pnpm/server@9.0.3

## 0.3.46

### Patch Changes

- Updated dependencies [09492b7b4]
  - @pnpm/package-store@10.1.8
  - @pnpm/server@9.0.3
  - @pnpm/client@2.0.12

## 0.3.45

### Patch Changes

- @pnpm/client@2.0.11
- @pnpm/package-store@10.1.7
- @pnpm/server@9.0.3

## 0.3.44

### Patch Changes

- Updated dependencies [01aecf038]
  - @pnpm/package-store@10.1.6
  - @pnpm/server@9.0.3
  - @pnpm/client@2.0.10

## 0.3.43

### Patch Changes

- @pnpm/cli-meta@1.0.1
- @pnpm/config@11.7.1
- @pnpm/package-store@10.1.5
- @pnpm/server@9.0.3
- @pnpm/client@2.0.9

## 0.3.42

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0

## 0.3.41

### Patch Changes

- @pnpm/cli-meta@1.0.1
- @pnpm/config@11.6.1
- @pnpm/package-store@10.1.4
- @pnpm/server@9.0.2
- @pnpm/client@2.0.8

## 0.3.40

### Patch Changes

- Updated dependencies [f591fdeeb]
- Updated dependencies [3a83db407]
  - @pnpm/config@11.6.0
  - @pnpm/client@2.0.7
  - @pnpm/package-store@10.1.3
  - @pnpm/server@9.0.1

## 0.3.39

### Patch Changes

- @pnpm/client@2.0.6
- @pnpm/package-store@10.1.2
- @pnpm/server@9.0.1

## 0.3.38

### Patch Changes

- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0

## 0.3.37

### Patch Changes

- @pnpm/client@2.0.5
- @pnpm/package-store@10.1.1
- @pnpm/server@9.0.1

## 0.3.36

### Patch Changes

- Updated dependencies [23cf3c88b]
  - @pnpm/config@11.4.0

## 0.3.35

### Patch Changes

- Updated dependencies [0a6544043]
  - @pnpm/package-store@10.1.0
  - @pnpm/server@9.0.1
  - @pnpm/client@2.0.4

## 0.3.34

### Patch Changes

- Updated dependencies [d94b19b39]
  - @pnpm/package-store@10.0.2
  - @pnpm/server@9.0.0

## 0.3.33

### Patch Changes

- Updated dependencies [7f74cd173]
- Updated dependencies [767212f4e]
- Updated dependencies [092f8dd83]
  - @pnpm/package-store@10.0.1
  - @pnpm/config@11.3.0
  - @pnpm/server@9.0.0

## 0.3.32

### Patch Changes

- Updated dependencies [86cd72de3]
  - @pnpm/package-store@10.0.0
  - @pnpm/server@9.0.0
  - @pnpm/client@2.0.3

## 0.3.31

### Patch Changes

- Updated dependencies [6457562c4]
- Updated dependencies [6457562c4]
  - @pnpm/package-store@9.1.8
  - @pnpm/server@8.0.9
  - @pnpm/client@2.0.2

## 0.3.30

### Patch Changes

- @pnpm/package-store@9.1.7
- @pnpm/server@8.0.8

## 0.3.29

### Patch Changes

- Updated dependencies [75a36deba]
- Updated dependencies [9f1a29ff9]
  - @pnpm/error@1.3.1
  - @pnpm/config@11.2.7
  - @pnpm/client@2.0.1
  - @pnpm/package-store@9.1.6
  - @pnpm/server@8.0.8

## 0.3.28

### Patch Changes

- Updated dependencies [ac0d3e122]
  - @pnpm/config@11.2.6

## 0.3.27

### Patch Changes

- Updated dependencies [855f8b00a]
- Updated dependencies [972864e0d]
- Updated dependencies [a1cdae3dc]
  - @pnpm/client@2.0.0
  - @pnpm/config@11.2.5
  - @pnpm/package-store@9.1.5
  - @pnpm/server@8.0.8

## 0.3.26

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/package-store@9.1.4
  - @pnpm/config@11.2.4
  - @pnpm/client@1.0.7
  - @pnpm/server@8.0.8

## 0.3.25

### Patch Changes

- Updated dependencies [13c18e397]
  - @pnpm/config@11.2.3

## 0.3.24

### Patch Changes

- Updated dependencies [3f6d35997]
  - @pnpm/config@11.2.2

## 0.3.23

### Patch Changes

- @pnpm/client@1.0.6
- @pnpm/package-store@9.1.3
- @pnpm/server@8.0.7

## 0.3.22

### Patch Changes

- @pnpm/client@1.0.5
- @pnpm/package-store@9.1.2
- @pnpm/server@8.0.7

## 0.3.21

### Patch Changes

- Updated dependencies [a2ef8084f]
  - @pnpm/cli-meta@1.0.1
  - @pnpm/config@11.2.1
  - @pnpm/package-store@9.1.1
  - @pnpm/client@1.0.4
  - @pnpm/server@8.0.7

## 0.3.20

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/config@11.2.0

## 0.3.19

### Patch Changes

- Updated dependencies [9a908bc07]
  - @pnpm/package-store@9.1.0
  - @pnpm/client@1.0.3
  - @pnpm/server@8.0.7

## 0.3.18

### Patch Changes

- 7b98d16c8: Update lru-cache to v6
- Updated dependencies [65b4d07ca]
- Updated dependencies [ab3b8f51d]
  - @pnpm/config@11.1.0
  - @pnpm/client@1.0.2
  - @pnpm/server@8.0.6
  - @pnpm/package-store@9.0.14

## 0.3.17

### Patch Changes

- d9310c034: Replace diable with a fork that has fewer dependencies.
  - @pnpm/client@1.0.1
  - @pnpm/package-store@9.0.13
  - @pnpm/server@8.0.5

## 0.3.16

### Patch Changes

- @pnpm/config@11.0.1

## 0.3.15

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [71aeb9a38]
- Updated dependencies [915828b46]
  - @pnpm/config@11.0.0
  - @pnpm/client@1.0.0
  - @pnpm/server@8.0.5
  - @pnpm/package-store@9.0.12

## 0.3.14

### Patch Changes

- @pnpm/default-fetcher@6.0.9
- @pnpm/package-store@9.0.11
- @pnpm/server@8.0.4

## 0.3.13

### Patch Changes

- @pnpm/config@10.0.1

## 0.3.12

### Patch Changes

- Updated dependencies [db17f6f7b]
- Updated dependencies [1146b76d2]
  - @pnpm/config@10.0.0
  - @pnpm/cli-meta@1.0.0
  - @pnpm/package-store@9.0.10
  - @pnpm/server@8.0.4
  - @pnpm/default-fetcher@6.0.8
  - @pnpm/default-resolver@9.0.3

## 0.3.11

### Patch Changes

- Updated dependencies [1adacd41e]
  - @pnpm/package-store@9.0.9
  - @pnpm/server@8.0.3

## 0.3.10

### Patch Changes

- @pnpm/default-resolver@9.0.2
- @pnpm/default-fetcher@6.0.7
- @pnpm/package-store@9.0.8
- @pnpm/server@8.0.3

## 0.3.9

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/config@9.2.0
  - @pnpm/cli-meta@1.0.0
  - @pnpm/package-store@9.0.7
  - @pnpm/server@8.0.3
  - @pnpm/default-fetcher@6.0.6
  - @pnpm/default-resolver@9.0.1

## 0.3.8

### Patch Changes

- @pnpm/package-store@9.0.6
- @pnpm/default-fetcher@6.0.5
- @pnpm/server@8.0.2

## 0.3.7

### Patch Changes

- Updated dependencies [41d92948b]
  - @pnpm/default-resolver@9.0.0
  - @pnpm/package-store@9.0.5
  - @pnpm/server@8.0.2

## 0.3.6

### Patch Changes

- Updated dependencies [d3ddd023c]
  - @pnpm/package-store@9.0.4
  - @pnpm/server@8.0.2
  - @pnpm/default-resolver@8.0.2
  - @pnpm/default-fetcher@6.0.4

## 0.3.5

### Patch Changes

- @pnpm/package-store@9.0.3
- @pnpm/server@8.0.1

## 0.3.4

### Patch Changes

- @pnpm/default-resolver@8.0.1
- @pnpm/package-store@9.0.2
- @pnpm/server@8.0.1
- @pnpm/default-fetcher@6.0.3

## 0.3.3

### Patch Changes

- Updated dependencies [1dcfecb36]
  - @pnpm/server@8.0.1

## 0.3.2

### Patch Changes

- Updated dependencies [ffddf34a8]
- Updated dependencies [429c5a560]
  - @pnpm/config@9.1.0
  - @pnpm/package-store@9.0.1
  - @pnpm/default-fetcher@6.0.2
  - @pnpm/server@8.0.0

## 0.3.1

### Patch Changes

- @pnpm/default-fetcher@6.0.1

## 0.3.0

### Minor Changes

- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- b6a82072e: Using a content-addressable filesystem for storing packages.
- 45fdcfde2: Locking is removed.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [242cf8737]
- Updated dependencies [cbc2192f1]
- Updated dependencies [f516d266c]
- Updated dependencies [ecf2c6b7d]
- Updated dependencies [da091c711]
- Updated dependencies [a7d20d927]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [c207d994f]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [919103471]
  - @pnpm/package-store@9.0.0
  - @pnpm/server@8.0.0
  - @pnpm/config@9.0.0
  - @pnpm/default-fetcher@6.0.0
  - @pnpm/cli-meta@1.0.0
  - @pnpm/default-resolver@7.4.10
  - @pnpm/error@1.2.1

## 0.3.0-alpha.5

### Minor Changes

- 45fdcfde2: Locking is removed.

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [a7d20d927]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/package-store@9.0.0-alpha.5
  - @pnpm/server@8.0.0-alpha.5
  - @pnpm/default-fetcher@5.1.19-alpha.5

## 0.3.0-alpha.4

### Minor Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.

### Patch Changes

- Updated dependencies [ecf2c6b7]
- Updated dependencies [da091c71]
  - @pnpm/package-store@9.0.0-alpha.4
  - @pnpm/server@8.0.0-alpha.4
  - @pnpm/default-fetcher@5.1.19-alpha.4
  - @pnpm/cli-meta@1.0.0-alpha.0
  - @pnpm/config@8.3.1-alpha.1
  - @pnpm/default-resolver@7.4.10-alpha.2

## 0.3.0-alpha.3

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/package-store@9.0.0-alpha.3
  - @pnpm/server@8.0.0-alpha.3
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/default-resolver@7.4.10-alpha.1
  - @pnpm/default-fetcher@5.1.19-alpha.3

## 0.2.32-alpha.2

### Patch Changes

- Updated dependencies [c207d994f]
- Updated dependencies [919103471]
  - @pnpm/package-store@9.0.0-alpha.2
  - @pnpm/server@8.0.0-alpha.2
  - @pnpm/default-fetcher@5.1.19-alpha.2
  - @pnpm/default-resolver@7.4.10-alpha.0

## 0.3.0-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/package-store@9.0.0-alpha.1
  - @pnpm/server@7.0.5-alpha.1
  - @pnpm/default-fetcher@5.1.19-alpha.1

## 0.3.0-alpha.0

### Minor Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/default-fetcher@6.0.0-alpha.0
  - @pnpm/package-store@9.0.0-alpha.0
  - @pnpm/server@8.0.0-alpha.0

## 0.2.31

### Patch Changes

- 907c63a48: Update `@pnpm/store-path`.
- 907c63a48: Dependencies updated.
- 907c63a48: Use `fs.mkdir` instead of `make-dir`.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/package-store@8.1.0
  - @pnpm/server@7.0.4
  - @pnpm/default-fetcher@5.1.18
  - @pnpm/default-resolver@7.4.9
