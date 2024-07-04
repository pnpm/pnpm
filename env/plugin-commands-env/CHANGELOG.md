# @pnpm/plugin-commands-env

## 5.0.11

### Patch Changes

- Updated dependencies [1b03682]
  - @pnpm/config@21.6.0
  - @pnpm/cli-utils@3.1.3
  - @pnpm/fetch@8.0.3
  - @pnpm/remove-bins@6.0.4
  - @pnpm/node.fetcher@4.0.7
  - @pnpm/node.resolver@3.0.7

## 5.0.10

### Patch Changes

- Updated dependencies [7c6c923]
- Updated dependencies [7d10394]
- Updated dependencies [d8eab39]
- Updated dependencies [04b8363]
  - @pnpm/config@21.5.0
  - @pnpm/cli-utils@3.1.2
  - @pnpm/fetch@8.0.2
  - @pnpm/remove-bins@6.0.3
  - @pnpm/node.fetcher@4.0.6
  - @pnpm/node.resolver@3.0.6

## 5.0.9

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/config@21.4.0
  - @pnpm/cli-utils@3.1.1
  - @pnpm/node.fetcher@4.0.5

## 5.0.8

### Patch Changes

- Updated dependencies [b7ca13f]
- Updated dependencies [b7ca13f]
  - @pnpm/cli-utils@3.1.0
  - @pnpm/config@21.3.0
  - @pnpm/node.fetcher@4.0.5
  - @pnpm/node.resolver@3.0.5

## 5.0.7

### Patch Changes

- @pnpm/config@21.2.3
- @pnpm/node.fetcher@4.0.4
- @pnpm/cli-utils@3.0.7
- @pnpm/node.resolver@3.0.4

## 5.0.6

### Patch Changes

- @pnpm/cli-utils@3.0.6
- @pnpm/config@21.2.2
- @pnpm/fetch@8.0.1
- @pnpm/remove-bins@6.0.2
- @pnpm/node.fetcher@4.0.3
- @pnpm/node.resolver@3.0.3

## 5.0.5

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/cli-utils@3.0.5
  - @pnpm/config@21.2.1
  - @pnpm/node.fetcher@4.0.2
  - @pnpm/store-path@9.0.1
  - @pnpm/node.resolver@3.0.2
  - @pnpm/remove-bins@6.0.1

## 5.0.4

### Patch Changes

- @pnpm/cli-utils@3.0.4

## 5.0.3

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/config@21.2.0
  - @pnpm/cli-utils@3.0.3
  - @pnpm/node.fetcher@4.0.1

## 5.0.2

### Patch Changes

- Updated dependencies [a80b539]
  - @pnpm/cli-utils@3.0.2
  - @pnpm/node.fetcher@4.0.1
  - @pnpm/node.resolver@3.0.1

## 5.0.1

### Patch Changes

- Updated dependencies [e0f47f4]
  - @pnpm/config@21.1.0
  - @pnpm/cli-utils@3.0.1

## 5.0.0

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
- Updated dependencies [43cdd87]
- Updated dependencies [2d9e3b8]
- Updated dependencies [3477ee5]
- Updated dependencies [cfa33f1]
- Updated dependencies [e748162]
- Updated dependencies [2b89155]
- Updated dependencies [60839fc]
- Updated dependencies [730929e]
- Updated dependencies [98566d9]
  - @pnpm/config@21.0.0
  - @pnpm/fetch@8.0.0
  - @pnpm/error@6.0.0
  - @pnpm/remove-bins@6.0.0
  - @pnpm/node.resolver@3.0.0
  - @pnpm/node.fetcher@4.0.0
  - @pnpm/store-path@9.0.0
  - @pnpm/cli-utils@3.0.0

## 4.1.22

### Patch Changes

- @pnpm/cli-utils@2.1.9
- @pnpm/config@20.4.2
- @pnpm/node.fetcher@3.0.39
- @pnpm/node.resolver@2.0.40

## 4.1.21

### Patch Changes

- @pnpm/node.fetcher@3.0.38
- @pnpm/node.resolver@2.0.39

## 4.1.20

### Patch Changes

- Updated dependencies [37ccff637]
- Updated dependencies [d9564e354]
  - @pnpm/store-path@8.0.2
  - @pnpm/config@20.4.1
  - @pnpm/cli-utils@2.1.8
  - @pnpm/node.fetcher@3.0.37

## 4.1.19

### Patch Changes

- @pnpm/node.fetcher@3.0.37
- @pnpm/node.resolver@2.0.38

## 4.1.18

### Patch Changes

- Updated dependencies [c597f72ec]
  - @pnpm/config@20.4.0
  - @pnpm/cli-utils@2.1.7

## 4.1.17

### Patch Changes

- 4d2923858: Fix error message texts in the `pnpm env` commands [#7456](https://github.com/pnpm/pnpm/pull/7456).
- Updated dependencies [4e71066dd]
- Updated dependencies [33313d2fd]
  - @pnpm/config@20.3.0
  - @pnpm/node.fetcher@3.0.36
  - @pnpm/cli-utils@2.1.6
  - @pnpm/node.resolver@2.0.37
  - @pnpm/remove-bins@5.0.7
  - @pnpm/fetch@7.0.7

## 4.1.16

### Patch Changes

- Updated dependencies [672c559e4]
  - @pnpm/config@20.2.0
  - @pnpm/cli-utils@2.1.5
  - @pnpm/remove-bins@5.0.6
  - @pnpm/node.fetcher@3.0.35
  - @pnpm/fetch@7.0.6
  - @pnpm/node.resolver@2.0.36

## 4.1.15

### Patch Changes

- @pnpm/node.fetcher@3.0.34
- @pnpm/node.resolver@2.0.35

## 4.1.14

### Patch Changes

- b9c7fb91f: chore: unify expressions
  - @pnpm/cli-utils@2.1.4

## 4.1.13

### Patch Changes

- @pnpm/cli-utils@2.1.3

## 4.1.12

### Patch Changes

- @pnpm/node.fetcher@3.0.33
- @pnpm/node.resolver@2.0.34

## 4.1.11

### Patch Changes

- @pnpm/node.fetcher@3.0.32
- @pnpm/node.resolver@2.0.33
- @pnpm/config@20.1.2
- @pnpm/cli-utils@2.1.2

## 4.1.10

### Patch Changes

- @pnpm/node.fetcher@3.0.31
- @pnpm/node.resolver@2.0.32

## 4.1.9

### Patch Changes

- Updated dependencies [7d65d901a]
  - @pnpm/store-path@8.0.1
  - @pnpm/node.fetcher@3.0.30
  - @pnpm/node.resolver@2.0.31
  - @pnpm/config@20.1.1
  - @pnpm/cli-utils@2.1.1

## 4.1.8

### Patch Changes

- @pnpm/node.fetcher@3.0.29
- @pnpm/node.resolver@2.0.30

## 4.1.7

### Patch Changes

- Updated dependencies [43ce9e4a6]
- Updated dependencies [d6592964f]
  - @pnpm/cli-utils@2.1.0
  - @pnpm/config@20.1.0
  - @pnpm/remove-bins@5.0.5
  - @pnpm/node.fetcher@3.0.28
  - @pnpm/fetch@7.0.5
  - @pnpm/node.resolver@2.0.29

## 4.1.6

### Patch Changes

- @pnpm/node.fetcher@3.0.27
- @pnpm/node.resolver@2.0.28

## 4.1.5

### Patch Changes

- @pnpm/node.fetcher@3.0.26
- @pnpm/node.resolver@2.0.27

## 4.1.4

### Patch Changes

- Updated dependencies [ac5abd3ff]
- Updated dependencies [b60bb6cbe]
  - @pnpm/config@20.0.0
  - @pnpm/cli-utils@2.0.24
  - @pnpm/node.fetcher@3.0.25
  - @pnpm/node.resolver@2.0.26

## 4.1.3

### Patch Changes

- @pnpm/node.fetcher@3.0.24
- @pnpm/node.resolver@2.0.25

## 4.1.2

### Patch Changes

- @pnpm/node.fetcher@3.0.23
- @pnpm/node.resolver@2.0.24

## 4.1.1

### Patch Changes

- Updated dependencies [b1dd0ee58]
  - @pnpm/config@19.2.1
  - @pnpm/cli-utils@2.0.23

## 4.1.0

### Minor Changes

- 2e6915727: Allow `env rm` to remove multiple node versions at once, and introduce `env add` for installing node versions without setting as default [#7155](https://github.com/pnpm/pnpm/pull/7155).

### Patch Changes

- Updated dependencies [d774a3196]
- Updated dependencies [832e28826]
  - @pnpm/config@19.2.0
  - @pnpm/cli-utils@2.0.22
  - @pnpm/remove-bins@5.0.4
  - @pnpm/node.fetcher@3.0.22
  - @pnpm/fetch@7.0.4
  - @pnpm/node.resolver@2.0.23

## 4.0.30

### Patch Changes

- Updated dependencies [ee328fd25]
  - @pnpm/config@19.1.0
  - @pnpm/cli-utils@2.0.21
  - @pnpm/node.fetcher@3.0.21
  - @pnpm/node.resolver@2.0.22

## 4.0.29

### Patch Changes

- @pnpm/cli-utils@2.0.20
- @pnpm/node.fetcher@3.0.20
- @pnpm/node.resolver@2.0.21

## 4.0.28

### Patch Changes

- Updated dependencies [9caa33d53]
  - @pnpm/node.fetcher@4.0.0
  - @pnpm/node.resolver@2.0.20
  - @pnpm/config@19.0.3
  - @pnpm/cli-utils@2.0.19

## 4.0.27

### Patch Changes

- @pnpm/node.fetcher@3.0.18
- @pnpm/node.resolver@2.0.19

## 4.0.26

### Patch Changes

- @pnpm/node.fetcher@3.0.17
- @pnpm/config@19.0.2
- @pnpm/node.resolver@2.0.18
- @pnpm/cli-utils@2.0.18

## 4.0.25

### Patch Changes

- @pnpm/node.fetcher@3.0.16
- @pnpm/node.resolver@2.0.17
- @pnpm/config@19.0.1

## 4.0.24

### Patch Changes

- @pnpm/node.fetcher@3.0.15
- @pnpm/node.resolver@2.0.16
- @pnpm/config@19.0.1
- @pnpm/cli-utils@2.0.17

## 4.0.23

### Patch Changes

- Updated dependencies [cb8bcc8df]
- Updated dependencies [66423df83]
  - @pnpm/config@19.0.0
  - @pnpm/node.fetcher@3.0.14
  - @pnpm/cli-utils@2.0.16
  - @pnpm/node.resolver@2.0.15

## 4.0.22

### Patch Changes

- 34724dd0f: `pnpm env use` should retry deleting the previous node.js executable [#6587](https://github.com/pnpm/pnpm/issues/6587).
  - @pnpm/cli-utils@2.0.15
  - @pnpm/node.fetcher@3.0.13
  - @pnpm/node.resolver@2.0.14

## 4.0.21

### Patch Changes

- @pnpm/node.fetcher@3.0.12
- @pnpm/node.resolver@2.0.13

## 4.0.20

### Patch Changes

- @pnpm/cli-utils@2.0.14

## 4.0.19

### Patch Changes

- @pnpm/node.fetcher@3.0.11
- @pnpm/node.resolver@2.0.12
- @pnpm/config@18.4.4

## 4.0.18

### Patch Changes

- @pnpm/node.fetcher@3.0.10
- @pnpm/node.resolver@2.0.11
- @pnpm/config@18.4.4

## 4.0.17

### Patch Changes

- @pnpm/node.fetcher@3.0.9
- @pnpm/node.resolver@2.0.10
- @pnpm/config@18.4.4

## 4.0.16

### Patch Changes

- @pnpm/cli-utils@2.0.13
- @pnpm/config@18.4.4
- @pnpm/remove-bins@5.0.3
- @pnpm/node.fetcher@3.0.8
- @pnpm/fetch@7.0.3
- @pnpm/node.resolver@2.0.9

## 4.0.15

### Patch Changes

- @pnpm/cli-utils@2.0.12
- @pnpm/config@18.4.3
- @pnpm/node.fetcher@3.0.7
- @pnpm/node.resolver@2.0.8

## 4.0.14

### Patch Changes

- @pnpm/node.fetcher@3.0.6
- @pnpm/node.resolver@2.0.7
- @pnpm/config@18.4.2

## 4.0.13

### Patch Changes

- Updated dependencies [e2d631217]
  - @pnpm/config@18.4.2
  - @pnpm/cli-utils@2.0.11
  - @pnpm/node.fetcher@3.0.5
  - @pnpm/node.resolver@2.0.6

## 4.0.12

### Patch Changes

- @pnpm/config@18.4.1
- @pnpm/error@5.0.2
- @pnpm/cli-utils@2.0.10
- @pnpm/node.fetcher@3.0.4
- @pnpm/node.resolver@2.0.5
- @pnpm/remove-bins@5.0.2

## 4.0.11

### Patch Changes

- @pnpm/node.fetcher@3.0.3
- @pnpm/node.resolver@2.0.4
- @pnpm/config@18.4.0

## 4.0.10

### Patch Changes

- Updated dependencies [301b8e2da]
  - @pnpm/config@18.4.0
  - @pnpm/cli-utils@2.0.9
  - @pnpm/remove-bins@5.0.1
  - @pnpm/error@5.0.1
  - @pnpm/node.fetcher@3.0.2
  - @pnpm/fetch@7.0.2
  - @pnpm/node.resolver@2.0.3

## 4.0.9

### Patch Changes

- Updated dependencies [ee429b300]
- Updated dependencies [1de07a4af]
  - @pnpm/cli-utils@2.0.8
  - @pnpm/config@18.3.2

## 4.0.8

### Patch Changes

- Updated dependencies [2809e89ab]
  - @pnpm/config@18.3.1
  - @pnpm/cli-utils@2.0.7
  - @pnpm/node.fetcher@3.0.1

## 4.0.7

### Patch Changes

- Updated dependencies [8228c2cb1]
  - @pnpm/node.fetcher@3.0.1
  - @pnpm/fetch@7.0.1
  - @pnpm/node.resolver@2.0.2

## 4.0.6

### Patch Changes

- 49b15ac2e: Use hard links to link the node executable on Windows machines [#4315](https://github.com/pnpm/pnpm/issues/4315).
- c0760128d: bump semver to 7.4.0
- Updated dependencies [32f8e08c6]
- Updated dependencies [c0760128d]
  - @pnpm/config@18.3.0
  - @pnpm/node.resolver@2.0.1
  - @pnpm/cli-utils@2.0.6

## 4.0.5

### Patch Changes

- Updated dependencies [fc8780ca9]
  - @pnpm/config@18.2.0
  - @pnpm/cli-utils@2.0.5

## 4.0.4

### Patch Changes

- @pnpm/cli-utils@2.0.4
- @pnpm/config@18.1.1

## 4.0.3

### Patch Changes

- Updated dependencies [e2cb4b63d]
- Updated dependencies [cd6ce11f0]
  - @pnpm/config@18.1.0
  - @pnpm/cli-utils@2.0.3

## 4.0.2

### Patch Changes

- @pnpm/config@18.0.2
- @pnpm/cli-utils@2.0.2

## 4.0.1

### Patch Changes

- @pnpm/config@18.0.1
- @pnpm/cli-utils@2.0.1

## 4.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [158d8cf22]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [8e35c21d1]
- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [113f0ae26]
  - @pnpm/config@18.0.0
  - @pnpm/remove-bins@5.0.0
  - @pnpm/node.resolver@2.0.0
  - @pnpm/node.fetcher@3.0.0
  - @pnpm/store-path@8.0.0
  - @pnpm/error@5.0.0
  - @pnpm/cli-utils@2.0.0
  - @pnpm/fetch@7.0.0

## 3.1.36

### Patch Changes

- 685b3a7ea: New directories should be prepended to NODE_PATH in command shims, not appended.
  - @pnpm/config@17.0.2
  - @pnpm/cli-utils@1.1.7

## 3.1.35

### Patch Changes

- Updated dependencies [b38d711f3]
  - @pnpm/config@17.0.1
  - @pnpm/cli-utils@1.1.6

## 3.1.34

### Patch Changes

- Updated dependencies [e505b58e3]
  - @pnpm/config@17.0.0
  - @pnpm/cli-utils@1.1.5
  - @pnpm/node.fetcher@2.0.14
  - @pnpm/node.resolver@1.1.11

## 3.1.33

### Patch Changes

- @pnpm/config@16.7.2
- @pnpm/cli-utils@1.1.4

## 3.1.32

### Patch Changes

- @pnpm/config@16.7.1
- @pnpm/cli-utils@1.1.3

## 3.1.31

### Patch Changes

- e570adc10: `pnpm env -g` should fail with a meaningful error message if pnpm cannot find the pnpm home directory, which is the directory into which Node.js is installed.
- Updated dependencies [7d64d757b]
- Updated dependencies [5c31fa8be]
  - @pnpm/cli-utils@1.1.2
  - @pnpm/config@16.7.0

## 3.1.30

### Patch Changes

- @pnpm/config@16.6.4
- @pnpm/cli-utils@1.1.1

## 3.1.29

### Patch Changes

- Updated dependencies [0377d9367]
  - @pnpm/cli-utils@1.1.0
  - @pnpm/config@16.6.3

## 3.1.28

### Patch Changes

- Updated dependencies [673e23060]
- Updated dependencies [9fa6c7404]
  - @pnpm/fetch@6.0.6
  - @pnpm/node.fetcher@2.0.13
  - @pnpm/node.resolver@1.1.10
  - @pnpm/config@16.6.2
  - @pnpm/cli-utils@1.0.34

## 3.1.27

### Patch Changes

- @pnpm/config@16.6.1
- @pnpm/node.fetcher@2.0.12
- @pnpm/node.resolver@1.1.9
- @pnpm/cli-utils@1.0.33

## 3.1.26

### Patch Changes

- Updated dependencies [59ee53678]
  - @pnpm/config@16.6.0
  - @pnpm/cli-utils@1.0.32

## 3.1.25

### Patch Changes

- @pnpm/config@16.5.5
- @pnpm/cli-utils@1.0.31

## 3.1.24

### Patch Changes

- @pnpm/node.fetcher@2.0.11
- @pnpm/node.resolver@1.1.8
- @pnpm/config@16.5.4
- @pnpm/cli-utils@1.0.30

## 3.1.23

### Patch Changes

- @pnpm/config@16.5.3
- @pnpm/cli-utils@1.0.29

## 3.1.22

### Patch Changes

- @pnpm/config@16.5.2
- @pnpm/cli-utils@1.0.28

## 3.1.21

### Patch Changes

- @pnpm/node.fetcher@2.0.10
- @pnpm/node.resolver@1.1.7
- @pnpm/config@16.5.1
- @pnpm/cli-utils@1.0.27

## 3.1.20

### Patch Changes

- Updated dependencies [28b47a156]
  - @pnpm/config@16.5.0
  - @pnpm/cli-utils@1.0.26

## 3.1.19

### Patch Changes

- @pnpm/node.fetcher@2.0.9
- @pnpm/node.resolver@1.1.6
- @pnpm/config@16.4.3
- @pnpm/cli-utils@1.0.25

## 3.1.18

### Patch Changes

- @pnpm/config@16.4.2
- @pnpm/cli-utils@1.0.24

## 3.1.17

### Patch Changes

- @pnpm/config@16.4.1
- @pnpm/cli-utils@1.0.23

## 3.1.16

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/config@16.4.0
  - @pnpm/node.fetcher@2.0.8
  - @pnpm/error@4.0.1
  - @pnpm/cli-utils@1.0.22
  - @pnpm/node.resolver@1.1.5
  - @pnpm/remove-bins@4.0.5

## 3.1.15

### Patch Changes

- Updated dependencies [1fad508b0]
  - @pnpm/config@16.3.0
  - @pnpm/cli-utils@1.0.21

## 3.1.14

### Patch Changes

- Updated dependencies [ec97a3105]
  - @pnpm/node.fetcher@2.0.7
  - @pnpm/cli-utils@1.0.20
  - @pnpm/node.resolver@1.1.4
  - @pnpm/config@16.2.2

## 3.1.13

### Patch Changes

- Updated dependencies [d71dbf230]
  - @pnpm/config@16.2.1
  - @pnpm/cli-utils@1.0.19

## 3.1.12

### Patch Changes

- 4700b095e: `pnpm env` should print help.
- Updated dependencies [841f52e70]
  - @pnpm/config@16.2.0
  - @pnpm/node.fetcher@2.0.6
  - @pnpm/cli-utils@1.0.18
  - @pnpm/node.resolver@1.1.3

## 3.1.11

### Patch Changes

- bc18d33fe: Allow the `-S` flag in command shims [pnpm/cmd-shim#42](https://github.com/pnpm/cmd-shim/pull/42).
  - @pnpm/cli-utils@1.0.17
  - @pnpm/config@16.1.11
  - @pnpm/remove-bins@4.0.4
  - @pnpm/node.fetcher@2.0.5
  - @pnpm/fetch@6.0.5
  - @pnpm/node.resolver@1.1.2

## 3.1.10

### Patch Changes

- @pnpm/config@16.1.10
- @pnpm/cli-utils@1.0.16

## 3.1.9

### Patch Changes

- @pnpm/config@16.1.9
- @pnpm/cli-utils@1.0.15

## 3.1.8

### Patch Changes

- @pnpm/cli-utils@1.0.14
- @pnpm/config@16.1.8

## 3.1.7

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/config@16.1.7
  - @pnpm/fetch@6.0.4
  - @pnpm/cli-utils@1.0.13
  - @pnpm/node.fetcher@2.0.4
  - @pnpm/node.resolver@1.1.1
  - @pnpm/remove-bins@4.0.3

## 3.1.6

### Patch Changes

- @pnpm/config@16.1.6
- @pnpm/cli-utils@1.0.12

## 3.1.5

### Patch Changes

- @pnpm/config@16.1.5
- @pnpm/cli-utils@1.0.11

## 3.1.4

### Patch Changes

- @pnpm/cli-utils@1.0.10
- @pnpm/config@16.1.4

## 3.1.3

### Patch Changes

- @pnpm/config@16.1.3
- @pnpm/cli-utils@1.0.9

## 3.1.2

### Patch Changes

- @pnpm/config@16.1.2
- @pnpm/cli-utils@1.0.8

## 3.1.1

### Patch Changes

- @pnpm/config@16.1.1
- @pnpm/cli-utils@1.0.7

## 3.1.0

### Minor Changes

- f60d6c46f: Support `pnpm env list` to list global or remote Node.js versions [#5546](https://github.com/pnpm/pnpm/issues/5546).

### Patch Changes

- Updated dependencies [3dab7f83c]
- Updated dependencies [f60d6c46f]
  - @pnpm/config@16.1.0
  - @pnpm/node.resolver@1.1.0
  - @pnpm/cli-utils@1.0.6

## 3.0.5

### Patch Changes

- @pnpm/cli-utils@1.0.5
- @pnpm/config@16.0.5
- @pnpm/remove-bins@4.0.2
- @pnpm/fetch@6.0.3
- @pnpm/node.fetcher@2.0.3
- @pnpm/node.resolver@1.0.19

## 3.0.4

### Patch Changes

- @pnpm/config@16.0.4
- @pnpm/cli-utils@1.0.4

## 3.0.3

### Patch Changes

- Updated dependencies [aacb83f73]
- Updated dependencies [a14ad09e6]
  - @pnpm/config@16.0.3
  - @pnpm/cli-utils@1.0.3

## 3.0.2

### Patch Changes

- Updated dependencies [bea0acdfc]
  - @pnpm/config@16.0.2
  - @pnpm/fetch@6.0.2
  - @pnpm/node.fetcher@2.0.2
  - @pnpm/node.resolver@1.0.18
  - @pnpm/cli-utils@1.0.2

## 3.0.1

### Patch Changes

- Updated dependencies [e7fd8a84c]
  - @pnpm/config@16.0.1
  - @pnpm/cli-utils@1.0.1
  - @pnpm/remove-bins@4.0.1
  - @pnpm/fetch@6.0.1
  - @pnpm/node.fetcher@2.0.1
  - @pnpm/node.resolver@1.0.17

## 3.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [1d0fd82fd]
- Updated dependencies [645384bfd]
- Updated dependencies [f884689e0]
- Updated dependencies [3c117996e]
  - @pnpm/config@16.0.0
  - @pnpm/error@4.0.0
  - @pnpm/fetch@6.0.0
  - @pnpm/cli-utils@1.0.0
  - @pnpm/node.fetcher@2.0.0
  - @pnpm/remove-bins@4.0.0
  - @pnpm/store-path@7.0.0
  - @pnpm/node.resolver@1.0.16

## 2.3.11

### Patch Changes

- @pnpm/config@15.10.12
- @pnpm/cli-utils@0.7.43

## 2.3.10

### Patch Changes

- @pnpm/fetch@5.0.10
- @pnpm/remove-bins@3.0.13
- @pnpm/cli-utils@0.7.42
- @pnpm/node.resolver@1.0.15
- @pnpm/config@15.10.11
- @pnpm/node.fetcher@1.0.15

## 2.3.9

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/cli-utils@0.7.41
  - @pnpm/config@15.10.10
  - @pnpm/node.fetcher@1.0.14
  - @pnpm/node.resolver@1.0.14
  - @pnpm/remove-bins@3.0.12

## 2.3.8

### Patch Changes

- @pnpm/config@15.10.9
- @pnpm/cli-utils@0.7.40
- @pnpm/remove-bins@3.0.11
- @pnpm/fetch@5.0.9
- @pnpm/node.fetcher@1.0.13
- @pnpm/node.resolver@1.0.13

## 2.3.7

### Patch Changes

- @pnpm/config@15.10.8
- @pnpm/cli-utils@0.7.39

## 2.3.6

### Patch Changes

- @pnpm/config@15.10.7
- @pnpm/cli-utils@0.7.38

## 2.3.5

### Patch Changes

- @pnpm/cli-utils@0.7.37
- @pnpm/config@15.10.6
- @pnpm/remove-bins@3.0.10
- @pnpm/fetch@5.0.8
- @pnpm/node.fetcher@1.0.12
- @pnpm/node.resolver@1.0.12

## 2.3.4

### Patch Changes

- @pnpm/config@15.10.5
- @pnpm/cli-utils@0.7.36

## 2.3.3

### Patch Changes

- @pnpm/cli-utils@0.7.35
- @pnpm/config@15.10.4

## 2.3.2

### Patch Changes

- @pnpm/config@15.10.3
- @pnpm/cli-utils@0.7.34

## 2.3.1

### Patch Changes

- @pnpm/config@15.10.2
- @pnpm/cli-utils@0.7.33

## 2.3.0

### Minor Changes

- ba270a967: Enhance `pnpm env` with the `remove` command.

### Patch Changes

- @pnpm/config@15.10.1
- @pnpm/cli-utils@0.7.32

## 2.2.1

### Patch Changes

- Updated dependencies [2aa22e4b1]
  - @pnpm/config@15.10.0
  - @pnpm/cli-utils@0.7.31

## 2.2.0

### Minor Changes

- c242672f5: Add an immediate response when fetching Node.js on version switching, preventing pnpm looks like just hanging there.

### Patch Changes

- Updated dependencies [1c7b439bb]
  - @pnpm/node.fetcher@1.0.11
  - @pnpm/node.resolver@1.0.11
  - @pnpm/config@15.9.4
  - @pnpm/cli-utils@0.7.30

## 2.1.30

### Patch Changes

- @pnpm/node.fetcher@1.0.10
- @pnpm/node.resolver@1.0.10
- @pnpm/config@15.9.3
- @pnpm/cli-utils@0.7.29

## 2.1.29

### Patch Changes

- @pnpm/node.fetcher@1.0.9
- @pnpm/node.resolver@1.0.9
- @pnpm/config@15.9.2
- @pnpm/cli-utils@0.7.28

## 2.1.28

### Patch Changes

- Updated dependencies [32915f0e4]
- Updated dependencies [7a17f99ab]
  - @pnpm/node.fetcher@1.0.8
  - @pnpm/node.resolver@1.0.8
  - @pnpm/config@15.9.1
  - @pnpm/cli-utils@0.7.27

## 2.1.27

### Patch Changes

- @pnpm/node.fetcher@1.0.7
- @pnpm/node.resolver@1.0.7
- @pnpm/config@15.9.0

## 2.1.26

### Patch Changes

- Updated dependencies [43cd6aaca]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
- Updated dependencies [29a81598a]
  - @pnpm/config@15.9.0
  - @pnpm/cli-utils@0.7.26
  - @pnpm/node.fetcher@1.0.6
  - @pnpm/node.resolver@1.0.6

## 2.1.25

### Patch Changes

- Updated dependencies [34121d753]
  - @pnpm/config@15.8.1
  - @pnpm/cli-utils@0.7.25
  - @pnpm/fetch@5.0.7
  - @pnpm/node.fetcher@1.0.5
  - @pnpm/node.resolver@1.0.5

## 2.1.24

### Patch Changes

- Updated dependencies [cac34ad69]
- Updated dependencies [99019e071]
  - @pnpm/config@15.8.0
  - @pnpm/cli-utils@0.7.24

## 2.1.23

### Patch Changes

- @pnpm/config@15.7.1
- @pnpm/cli-utils@0.7.23
- @pnpm/node.fetcher@1.0.4

## 2.1.22

### Patch Changes

- Updated dependencies [4fa1091c8]
  - @pnpm/config@15.7.0
  - @pnpm/cli-utils@0.7.22

## 2.1.21

### Patch Changes

- Updated dependencies [7334b347b]
  - @pnpm/config@15.6.1
  - @pnpm/cli-utils@0.7.21

## 2.1.20

### Patch Changes

- Updated dependencies [e018a8b14]
- Updated dependencies [28f000509]
- Updated dependencies [406656f80]
  - @pnpm/fetch@5.0.6
  - @pnpm/config@15.6.0
  - @pnpm/node.resolver@1.0.4
  - @pnpm/cli-utils@0.7.20

## 2.1.19

### Patch Changes

- @pnpm/config@15.5.2
- @pnpm/cli-utils@0.7.19

## 2.1.18

### Patch Changes

- Updated dependencies [2105735a0]
  - @pnpm/node.fetcher@1.0.4
  - @pnpm/cli-utils@0.7.18
  - @pnpm/node.resolver@1.0.4

## 2.1.17

### Patch Changes

- Updated dependencies [5f643f23b]
  - @pnpm/cli-utils@0.7.17
  - @pnpm/config@15.5.1
  - @pnpm/node.fetcher@1.0.3
  - @pnpm/node.resolver@1.0.3

## 2.1.16

### Patch Changes

- @pnpm/node.fetcher@1.0.2
- @pnpm/node.resolver@1.0.2

## 2.1.15

### Patch Changes

- Updated dependencies [f48d46ef6]
  - @pnpm/config@15.5.0
  - @pnpm/cli-utils@0.7.16

## 2.1.14

### Patch Changes

- @pnpm/cli-utils@0.7.15
- @pnpm/config@15.4.1
- @pnpm/fetch@5.0.5
- @pnpm/node.fetcher@1.0.1
- @pnpm/node.resolver@1.0.1

## 2.1.13

### Patch Changes

- d1df380ab: The `use-node-version` setting should work with prerelease Node.js versions. For instance:

  ```
  use-node-version=18.0.0-rc.3
  ```

- Updated dependencies [badbab154]
- Updated dependencies [228dcc3c9]
- Updated dependencies [47b5e45dd]
  - @pnpm/node.resolver@1.0.0
  - @pnpm/node.fetcher@1.0.0
  - @pnpm/config@15.4.0
  - @pnpm/cli-utils@0.7.14
  - @pnpm/fetch@5.0.4

## 2.1.12

### Patch Changes

- Updated dependencies [9d5bf09c0]
- Updated dependencies [56cf04cb3]
- Updated dependencies [0abfe1718]
- Updated dependencies [0abfe1718]
  - @pnpm/fetch@5.0.3
  - @pnpm/config@15.3.0
  - @pnpm/create-cafs-store@1.1.0
  - @pnpm/fetcher-base@12.1.0
  - @pnpm/cli-utils@0.7.13
  - @pnpm/tarball-fetcher@10.0.5

## 2.1.11

### Patch Changes

- Updated dependencies [25798aad1]
  - @pnpm/config@15.2.1
  - @pnpm/cli-utils@0.7.12

## 2.1.10

### Patch Changes

- Updated dependencies [bc80631d3]
- Updated dependencies [d5730ba81]
  - @pnpm/config@15.2.0
  - @pnpm/cli-utils@0.7.11
  - @pnpm/fetcher-base@12.0.3
  - @pnpm/fetch@5.0.2
  - @pnpm/create-cafs-store@1.0.3
  - @pnpm/tarball-fetcher@10.0.4

## 2.1.9

### Patch Changes

- Updated dependencies [6756c2b02]
  - @pnpm/fetcher-base@12.0.2
  - @pnpm/cli-utils@0.7.10
  - @pnpm/create-cafs-store@1.0.2
  - @pnpm/tarball-fetcher@10.0.3
  - @pnpm/config@15.1.4

## 2.1.8

### Patch Changes

- Updated dependencies [ae2f845c5]
  - @pnpm/config@15.1.4
  - @pnpm/cli-utils@0.7.9

## 2.1.7

### Patch Changes

- Updated dependencies [05159665d]
  - @pnpm/config@15.1.3
  - @pnpm/cli-utils@0.7.8

## 2.1.6

### Patch Changes

- @pnpm/cli-utils@0.7.7

## 2.1.5

### Patch Changes

- Updated dependencies [af22c6c4f]
  - @pnpm/config@15.1.2
  - @pnpm/cli-utils@0.7.6

## 2.1.4

### Patch Changes

- Updated dependencies [52b0576af]
  - @pnpm/cli-utils@0.7.5

## 2.1.3

### Patch Changes

- @pnpm/create-cafs-store@1.0.1
- @pnpm/cli-utils@0.7.4
- @pnpm/tarball-fetcher@10.0.2
- @pnpm/config@15.1.1

## 2.1.2

### Patch Changes

- Updated dependencies [1ceb632b1]
  - @pnpm/create-cafs-store@1.0.0
  - @pnpm/tarball-fetcher@10.0.2
  - @pnpm/cli-utils@0.7.3
  - @pnpm/config@15.1.1
  - @pnpm/fetcher-base@12.0.1
  - @pnpm/fetch@5.0.1

## 2.1.1

### Patch Changes

- Updated dependencies [e05dcc48a]
  - @pnpm/config@15.1.0
  - @pnpm/cli-utils@0.7.2

## 2.1.0

### Minor Changes

- 8fa95fd86: Path `extraNodePaths` to the bins linker.

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
  - @pnpm/package-store@13.0.1
  - @pnpm/cli-utils@0.7.1
  - @pnpm/error@3.0.1
  - @pnpm/tarball-fetcher@10.0.1

## 2.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [516859178]
- Updated dependencies [73d71a2d5]
- Updated dependencies [fa656992c]
- Updated dependencies [542014839]
- Updated dependencies [585e9ca9e]
  - @pnpm/config@14.0.0
  - @pnpm/error@3.0.0
  - @pnpm/fetch@5.0.0
  - @pnpm/fetcher-base@12.0.0
  - @pnpm/package-store@13.0.0
  - @pnpm/tarball-fetcher@10.0.0
  - @pnpm/cli-utils@0.7.0

## 1.4.14

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/cli-utils@0.6.50
  - @pnpm/config@13.13.2
  - @pnpm/tarball-fetcher@9.3.17
  - @pnpm/package-store@12.1.12

## 1.4.13

### Patch Changes

- @pnpm/package-store@12.1.11
- @pnpm/cli-utils@0.6.49
- @pnpm/config@13.13.1
- @pnpm/fetcher-base@11.1.6
- @pnpm/tarball-fetcher@9.3.16
- @pnpm/fetch@4.2.5

## 1.4.12

### Patch Changes

- Updated dependencies [fa4f9133b]
  - @pnpm/package-store@12.1.10
  - @pnpm/tarball-fetcher@9.3.15

## 1.4.11

### Patch Changes

- Updated dependencies [50e347d23]
  - @pnpm/package-store@12.1.9
  - @pnpm/tarball-fetcher@9.3.15

## 1.4.10

### Patch Changes

- Updated dependencies [334e5340a]
  - @pnpm/config@13.13.0
  - @pnpm/cli-utils@0.6.48

## 1.4.9

### Patch Changes

- Updated dependencies [b7566b979]
  - @pnpm/config@13.12.0
  - @pnpm/cli-utils@0.6.47

## 1.4.8

### Patch Changes

- Updated dependencies [fff0e4493]
  - @pnpm/config@13.11.0
  - @pnpm/cli-utils@0.6.46

## 1.4.7

### Patch Changes

- @pnpm/cli-utils@0.6.45

## 1.4.6

### Patch Changes

- Updated dependencies [e76151f66]
  - @pnpm/config@13.10.0
  - @pnpm/cli-utils@0.6.44
  - @pnpm/fetcher-base@11.1.5
  - @pnpm/package-store@12.1.8
  - @pnpm/fetch@4.2.4
  - @pnpm/tarball-fetcher@9.3.15

## 1.4.5

### Patch Changes

- @pnpm/cli-utils@0.6.43

## 1.4.4

### Patch Changes

- @pnpm/package-store@12.1.7
- @pnpm/tarball-fetcher@9.3.14

## 1.4.3

### Patch Changes

- Updated dependencies [8fe8f5e55]
  - @pnpm/config@13.9.0
  - @pnpm/cli-utils@0.6.42
  - @pnpm/package-store@12.1.6
  - @pnpm/tarball-fetcher@9.3.14

## 1.4.2

### Patch Changes

- Updated dependencies [732d4962f]
- Updated dependencies [a6cf11cb7]
  - @pnpm/config@13.8.0
  - @pnpm/package-store@12.1.6
  - @pnpm/cli-utils@0.6.41

## 1.4.1

### Patch Changes

- @pnpm/cli-utils@0.6.40
- @pnpm/config@13.7.2
- @pnpm/fetcher-base@11.1.4
- @pnpm/package-store@12.1.6
- @pnpm/fetch@4.2.3
- @pnpm/tarball-fetcher@9.3.14

## 1.4.0

### Minor Changes

- d16620cf9: If pnpm previously failed to install node when the `use-node-version` option is set, that download and install will now be re-attempted when pnpm is run again.

### Patch Changes

- @pnpm/cli-utils@0.6.39

## 1.3.1

### Patch Changes

- @pnpm/cli-utils@0.6.38
- @pnpm/config@13.7.1
- @pnpm/fetcher-base@11.1.3
- @pnpm/package-store@12.1.5
- @pnpm/tarball-fetcher@9.3.13
- @pnpm/fetch@4.2.2

## 1.3.0

### Minor Changes

- 10a4bd4db: New option added for: `node-mirror:<releaseDir>`. The string value of this dynamic option is used as the base URL for downloading node when `use-node-version` is specified. The `<releaseDir>` portion of this argument can be any dir in `https://nodejs.org/download`. Which `<releaseDir>` dynamic config option gets selected depends on the value of `use-node-version`. If 'use-node-version' is a simple `x.x.x` version string, `<releaseDir>` becomes `release` and `node-mirror:release` is read. Defaults to `https://nodejs.org/download/<releaseDir>/`.

### Patch Changes

- Updated dependencies [30bfca967]
- Updated dependencies [927c4a089]
- Updated dependencies [10a4bd4db]
- Updated dependencies [d00e1fc6a]
  - @pnpm/config@13.7.0
  - @pnpm/package-store@12.1.4
  - @pnpm/fetch@4.2.1
  - @pnpm/tarball-fetcher@9.3.12
  - @pnpm/cli-utils@0.6.37
  - @pnpm/fetcher-base@11.1.2

## 1.2.12

### Patch Changes

- Updated dependencies [b13e4b452]
  - @pnpm/tarball-fetcher@9.3.11
  - @pnpm/package-store@12.1.3

## 1.2.11

### Patch Changes

- Updated dependencies [f1c194ded]
- Updated dependencies [46aaf7108]
  - @pnpm/fetch@4.2.0
  - @pnpm/config@13.6.1
  - @pnpm/tarball-fetcher@9.3.10
  - @pnpm/cli-utils@0.6.36
  - @pnpm/package-store@12.1.3

## 1.2.10

### Patch Changes

- Updated dependencies [8a99a01ff]
  - @pnpm/config@13.6.0
  - @pnpm/cli-utils@0.6.35

## 1.2.9

### Patch Changes

- Updated dependencies [fb1a95a6c]
- Updated dependencies [fb1a95a6c]
  - @pnpm/tarball-fetcher@9.3.10
  - @pnpm/cli-utils@0.6.34
  - @pnpm/package-store@12.1.3

## 1.2.8

### Patch Changes

- Updated dependencies [a7ff2d5ce]
  - @pnpm/config@13.5.1
  - @pnpm/cli-utils@0.6.33
  - @pnpm/package-store@12.1.3
  - @pnpm/tarball-fetcher@9.3.9

## 1.2.7

### Patch Changes

- d4d7c4aee: `pnpm env use` should download the right Node.js tarball on Raspberry Pi [#4007](https://github.com/pnpm/pnpm/issues/4007).
- Updated dependencies [002778559]
- Updated dependencies [12ee3c144]
  - @pnpm/config@13.5.0
  - @pnpm/fetch@4.1.6
  - @pnpm/cli-utils@0.6.32
  - @pnpm/tarball-fetcher@9.3.9
  - @pnpm/package-store@12.1.2

## 1.2.6

### Patch Changes

- @pnpm/cli-utils@0.6.31
- @pnpm/package-store@12.1.2
- @pnpm/tarball-fetcher@9.3.9

## 1.2.5

### Patch Changes

- @pnpm/config@13.4.2
- @pnpm/cli-utils@0.6.30
- @pnpm/fetcher-base@11.1.1
- @pnpm/package-store@12.1.1
- @pnpm/fetch@4.1.5
- @pnpm/tarball-fetcher@9.3.9

## 1.2.4

### Patch Changes

- 6b7eb7249: Use the package manager's network and proxy configuration when making requests for Node.js.
  - @pnpm/package-store@12.1.0

## 1.2.3

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/fetcher-base@11.1.0
  - @pnpm/package-store@12.1.0
  - @pnpm/cli-utils@0.6.29
  - @pnpm/config@13.4.1
  - @pnpm/tarball-fetcher@9.3.8
  - @pnpm/fetch@4.1.4

## 1.2.2

### Patch Changes

- Updated dependencies [782ef2490]
  - @pnpm/fetch@4.1.3
  - @pnpm/tarball-fetcher@9.3.7
  - @pnpm/package-store@12.0.15

## 1.2.1

### Patch Changes

- Updated dependencies [b6d74c545]
  - @pnpm/config@13.4.0
  - @pnpm/cli-utils@0.6.28
  - @pnpm/package-store@12.0.15

## 1.2.0

### Minor Changes

- 37905fcf7: Install prerelease Node.js versions.
- 1a6cc7ee7: Allow to install the latest Node.js version by running `pnpm env use -g latest`.

### Patch Changes

- Updated dependencies [bd7bcdbe8]
  - @pnpm/config@13.3.0
  - @pnpm/fetch@4.1.2
  - @pnpm/cli-utils@0.6.27
  - @pnpm/tarball-fetcher@9.3.7
  - @pnpm/package-store@12.0.15

## 1.1.0

### Minor Changes

- 5ee3b2dc7: `pnpm env use` sets the `globalconfig` for npm CLI. The global config is located in a centralized place, so it persists after switching to a different Node.js or npm version.

### Patch Changes

- Updated dependencies [5ee3b2dc7]
  - @pnpm/config@13.2.0
  - @pnpm/cli-utils@0.6.26

## 1.0.10

### Patch Changes

- 913d97a05: Do not create a command shim for Node.js, just a symlink to the executable.
  - @pnpm/cli-utils@0.6.25

## 1.0.9

### Patch Changes

- Updated dependencies [4027a3c69]
  - @pnpm/config@13.1.0
  - @pnpm/cli-utils@0.6.24

## 1.0.8

### Patch Changes

- @pnpm/tarball-fetcher@9.3.7
- @pnpm/package-store@12.0.15

## 1.0.7

### Patch Changes

- 0d4a7c69e: Pick the right extension for command files. It is important to write files with .CMD extension on case sensitive Windows drives.
- Updated dependencies [fe5688dc0]
- Updated dependencies [c7081cbb4]
- Updated dependencies [c7081cbb4]
  - @pnpm/config@13.0.0
  - @pnpm/cli-utils@0.6.23

## 1.0.6

### Patch Changes

- Updated dependencies [d62259d67]
  - @pnpm/config@12.6.0
  - @pnpm/cli-utils@0.6.22

## 1.0.5

### Patch Changes

- Updated dependencies [6681fdcbc]
- Updated dependencies [bab172385]
  - @pnpm/config@12.5.0
  - @pnpm/fetch@4.1.1
  - @pnpm/cli-utils@0.6.21
  - @pnpm/package-store@12.0.15
  - @pnpm/tarball-fetcher@9.3.6

## 1.0.4

### Patch Changes

- Updated dependencies [eadf0e505]
  - @pnpm/fetch@4.1.0
  - @pnpm/tarball-fetcher@9.3.5
  - @pnpm/cli-utils@0.6.20
  - @pnpm/package-store@12.0.14

## 1.0.3

### Patch Changes

- 869b1afcb: Do not create powershell command shims for node, npm, and npx.

## 1.0.2

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9
  - @pnpm/cli-utils@0.6.19

## 1.0.1

### Patch Changes

- @pnpm/config@12.4.8
- @pnpm/cli-utils@0.6.18

## 1.0.0

### Major Changes

- 25a2d6e5c: When installing Node.js, also link the npm CLI that is bundled with Node.js.

## 0.2.13

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7
  - @pnpm/cli-utils@0.6.17
  - @pnpm/package-store@12.0.14
  - @pnpm/tarball-fetcher@9.3.4

## 0.2.12

### Patch Changes

- @pnpm/package-store@12.0.13
- @pnpm/tarball-fetcher@9.3.4

## 0.2.11

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6
  - @pnpm/cli-utils@0.6.16

## 0.2.10

### Patch Changes

- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5
  - @pnpm/cli-utils@0.6.15

## 0.2.9

### Patch Changes

- 27e6331c6: Allow to install a Node.js version using a semver range.
- af8b5716e: New command added: `pnpm env use --global <version>`. This command installs the specified Node.js version globally.
- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4
  - @pnpm/cli-utils@0.6.14

## 0.2.8

### Patch Changes

- @pnpm/config@12.4.3
- @pnpm/package-store@12.0.12
- @pnpm/fetch@4.0.2
- @pnpm/tarball-fetcher@9.3.4

## 0.2.7

### Patch Changes

- Updated dependencies [73c1f802e]
  - @pnpm/config@12.4.2

## 0.2.6

### Patch Changes

- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1

## 0.2.5

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0
  - @pnpm/package-store@12.0.11

## 0.2.4

### Patch Changes

- @pnpm/config@12.3.3
- @pnpm/package-store@12.0.11
- @pnpm/fetch@4.0.1
- @pnpm/tarball-fetcher@9.3.3

## 0.2.3

### Patch Changes

- @pnpm/package-store@12.0.10
- @pnpm/tarball-fetcher@9.3.2

## 0.2.2

### Patch Changes

- Updated dependencies [e7d9cd187]
- Updated dependencies [eeff424bd]
  - @pnpm/fetch@4.0.0
  - @pnpm/tarball-fetcher@9.3.2
  - @pnpm/package-store@12.0.9
  - @pnpm/config@12.3.2

## 0.2.1

### Patch Changes

- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1
  - @pnpm/tarball-fetcher@9.3.1
  - @pnpm/package-store@12.0.8

## 0.2.0

### Minor Changes

- c1f137412: Remove the `pnpm node [args...]` command.

### Patch Changes

- 6d2ccc9a3: Download Node.js from nodejs.org, not from the npm registry.
- Updated dependencies [6d2ccc9a3]
  - @pnpm/tarball-fetcher@9.3.0
  - @pnpm/package-store@12.0.7

## 0.1.0

### Minor Changes

- 84ec82e05: Project created.

### Patch Changes

- @pnpm/cli-utils@0.6.5
