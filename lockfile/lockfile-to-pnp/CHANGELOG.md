# @pnpm/lockfile-to-pnp

## 4.1.3

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-utils@11.0.0
  - @pnpm/dependency-path@5.0.0
  - @pnpm/lockfile-file@9.0.6

## 4.1.2

### Patch Changes

- @pnpm/lockfile-file@9.0.5

## 4.1.1

### Patch Changes

- Updated dependencies [7a0536e]
  - @pnpm/lockfile-utils@10.1.1
  - @pnpm/lockfile-file@9.0.4

## 4.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/dependency-path@4.0.0
  - @pnpm/lockfile-utils@10.1.0
  - @pnpm/lockfile-file@9.0.3

## 4.0.2

### Patch Changes

- Updated dependencies [c969f37]
  - @pnpm/lockfile-file@9.0.2

## 4.0.1

### Patch Changes

- Updated dependencies [2cbf7b7]
- Updated dependencies [6b6ca69]
  - @pnpm/lockfile-file@9.0.1

## 4.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- d381a60: Support for lockfile v5 is dropped. Use pnpm v8 to convert lockfile v5 to lockfile v6 [#7470](https://github.com/pnpm/pnpm/pull/7470).

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [cdd8365]
- Updated dependencies [89b396b]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [f67ad31]
- Updated dependencies [730929e]
- Updated dependencies [98a1266]
  - @pnpm/types@10.0.0
  - @pnpm/dependency-path@3.0.0
  - @pnpm/lockfile-utils@10.0.0
  - @pnpm/lockfile-file@9.0.0

## 3.0.17

### Patch Changes

- @pnpm/lockfile-utils@9.0.5

## 3.0.16

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2
  - @pnpm/lockfile-file@8.1.6
  - @pnpm/lockfile-utils@9.0.4
  - @pnpm/dependency-path@2.1.7

## 3.0.15

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/lockfile-file@8.1.5
  - @pnpm/lockfile-utils@9.0.3
  - @pnpm/dependency-path@2.1.6

## 3.0.14

### Patch Changes

- Updated dependencies [d5a176af7]
  - @pnpm/lockfile-utils@9.0.2

## 3.0.13

### Patch Changes

- Updated dependencies [b4194fe52]
  - @pnpm/lockfile-utils@9.0.1

## 3.0.12

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/lockfile-utils@9.0.0

## 3.0.11

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/lockfile-file@8.1.4
  - @pnpm/lockfile-utils@8.0.7
  - @pnpm/dependency-path@2.1.5

## 3.0.10

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/lockfile-file@8.1.3
  - @pnpm/lockfile-utils@8.0.6
  - @pnpm/dependency-path@2.1.4

## 3.0.9

### Patch Changes

- Updated dependencies [f394cfccd]
  - @pnpm/lockfile-utils@8.0.5

## 3.0.8

### Patch Changes

- Updated dependencies [e9aa6f682]
  - @pnpm/lockfile-utils@8.0.4

## 3.0.7

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/lockfile-file@8.1.2
  - @pnpm/lockfile-utils@8.0.3
  - @pnpm/dependency-path@2.1.3

## 3.0.6

### Patch Changes

- Updated dependencies [d9da627cd]
  - @pnpm/lockfile-utils@8.0.2
  - @pnpm/lockfile-file@8.1.1

## 3.0.5

### Patch Changes

- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
  - @pnpm/lockfile-file@8.1.0
  - @pnpm/types@9.1.0
  - @pnpm/lockfile-utils@8.0.1
  - @pnpm/dependency-path@2.1.2

## 3.0.4

### Patch Changes

- Updated dependencies [d58cdb962]
  - @pnpm/lockfile-utils@8.0.0

## 3.0.3

### Patch Changes

- Updated dependencies [c0760128d]
  - @pnpm/dependency-path@2.1.1
  - @pnpm/lockfile-file@8.0.2
  - @pnpm/lockfile-utils@7.0.1

## 3.0.2

### Patch Changes

- Updated dependencies [72ba638e3]
  - @pnpm/lockfile-utils@7.0.0

## 3.0.1

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
- Updated dependencies [94f94eed6]
  - @pnpm/dependency-path@2.1.0
  - @pnpm/lockfile-file@8.0.1
  - @pnpm/lockfile-utils@6.0.1

## 3.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [c92936158]
- Updated dependencies [158d8cf22]
- Updated dependencies [ca8f51e60]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [0e26acb0f]
- Updated dependencies [417c8ac59]
  - @pnpm/lockfile-utils@6.0.0
  - @pnpm/lockfile-file@8.0.0
  - @pnpm/dependency-path@2.0.0
  - @pnpm/types@9.0.0

## 2.0.14

### Patch Changes

- Updated dependencies [787c43dcc]
  - @pnpm/lockfile-file@7.0.6

## 2.0.13

### Patch Changes

- Updated dependencies [ed946c73e]
  - @pnpm/lockfile-file@7.0.5

## 2.0.12

### Patch Changes

- @pnpm/lockfile-utils@5.0.7

## 2.0.11

### Patch Changes

- Updated dependencies [d89d7a078]
  - @pnpm/dependency-path@1.1.3
  - @pnpm/lockfile-file@7.0.4
  - @pnpm/lockfile-utils@5.0.6

## 2.0.10

### Patch Changes

- Updated dependencies [9247f6781]
  - @pnpm/dependency-path@1.1.2
  - @pnpm/lockfile-file@7.0.3
  - @pnpm/lockfile-utils@5.0.5

## 2.0.9

### Patch Changes

- Updated dependencies [9a68ebbae]
  - @pnpm/lockfile-file@7.0.2

## 2.0.8

### Patch Changes

- Updated dependencies [0f6e95872]
  - @pnpm/dependency-path@1.1.1
  - @pnpm/lockfile-file@7.0.1
  - @pnpm/lockfile-utils@5.0.4

## 2.0.7

### Patch Changes

- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
  - @pnpm/lockfile-file@7.0.0
  - @pnpm/dependency-path@1.1.0
  - @pnpm/lockfile-utils@5.0.3

## 2.0.6

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/lockfile-file@6.0.5
  - @pnpm/lockfile-utils@5.0.2
  - @pnpm/dependency-path@1.0.1

## 2.0.5

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/dependency-path@1.0.0
  - @pnpm/lockfile-file@6.0.4
  - @pnpm/lockfile-utils@5.0.1

## 2.0.4

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/lockfile-file@6.0.3

## 2.0.3

### Patch Changes

- Updated dependencies [ecc8794bb]
- Updated dependencies [ecc8794bb]
  - @pnpm/lockfile-utils@5.0.0

## 2.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - dependency-path@9.2.8
  - @pnpm/lockfile-file@6.0.2
  - @pnpm/lockfile-utils@4.2.8

## 2.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - dependency-path@9.2.7
  - @pnpm/lockfile-file@6.0.1
  - @pnpm/lockfile-utils@4.2.7

## 2.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [f884689e0]
  - @pnpm/lockfile-file@6.0.0

## 1.0.5

### Patch Changes

- Updated dependencies [7c296fe9b]
  - @pnpm/lockfile-file@5.3.8

## 1.0.4

### Patch Changes

- @pnpm/lockfile-file@5.3.7

## 1.0.3

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - dependency-path@9.2.6
  - @pnpm/lockfile-file@5.3.6
  - @pnpm/lockfile-utils@4.2.6

## 1.0.2

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - dependency-path@9.2.5
  - @pnpm/lockfile-file@5.3.5
  - @pnpm/lockfile-utils@4.2.5

## 1.0.1

### Patch Changes

- Updated dependencies [0373af22e]
  - @pnpm/lockfile-file@5.3.4

## 1.0.0

### Major Changes

- 5035fdae1: Remove lockfileToPnp function.

### Patch Changes

- @pnpm/lockfile-utils@4.2.4

## 0.5.27

### Patch Changes

- Updated dependencies [1e5482da4]
  - @pnpm/lockfile-file@5.3.3
  - @pnpm/config@15.9.0

## 0.5.26

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [39c040127]
- Updated dependencies [43cd6aaca]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
- Updated dependencies [29a81598a]
  - @pnpm/read-project-manifest@3.0.9
  - @pnpm/config@15.9.0
  - @pnpm/lockfile-file@5.3.2
  - @pnpm/lockfile-utils@4.2.3

## 0.5.25

### Patch Changes

- Updated dependencies [44544b493]
- Updated dependencies [c90798461]
- Updated dependencies [34121d753]
  - @pnpm/lockfile-file@5.3.1
  - @pnpm/types@8.5.0
  - @pnpm/config@15.8.1
  - dependency-path@9.2.4
  - @pnpm/lockfile-utils@4.2.2
  - @pnpm/read-project-manifest@3.0.8

## 0.5.24

### Patch Changes

- Updated dependencies [c83f40c10]
  - @pnpm/lockfile-utils@4.2.1

## 0.5.23

### Patch Changes

- Updated dependencies [cac34ad69]
- Updated dependencies [99019e071]
  - @pnpm/config@15.8.0

## 0.5.22

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-file@5.3.0
  - @pnpm/lockfile-utils@4.2.0
  - @pnpm/config@15.7.1

## 0.5.21

### Patch Changes

- Updated dependencies [01c5834bf]
- Updated dependencies [4fa1091c8]
  - @pnpm/read-project-manifest@3.0.7
  - @pnpm/config@15.7.0
  - @pnpm/lockfile-file@5.2.0

## 0.5.20

### Patch Changes

- Updated dependencies [7334b347b]
- Updated dependencies [e3f4d131c]
  - @pnpm/config@15.6.1
  - @pnpm/lockfile-utils@4.1.0

## 0.5.19

### Patch Changes

- Updated dependencies [28f000509]
- Updated dependencies [406656f80]
  - @pnpm/config@15.6.0

## 0.5.18

### Patch Changes

- @pnpm/config@15.5.2

## 0.5.17

### Patch Changes

- dependency-path@9.2.3
- @pnpm/lockfile-utils@4.0.10

## 0.5.16

### Patch Changes

- Updated dependencies [ab684d77e]
  - @pnpm/lockfile-file@5.1.4

## 0.5.15

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/config@15.5.1
  - @pnpm/lockfile-file@5.1.3
  - @pnpm/lockfile-utils@4.0.9

## 0.5.14

### Patch Changes

- Updated dependencies [fc581d371]
  - dependency-path@9.2.2
  - @pnpm/lockfile-utils@4.0.8

## 0.5.13

### Patch Changes

- Updated dependencies [f48d46ef6]
  - @pnpm/config@15.5.0

## 0.5.12

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/lockfile-file@5.1.2
  - @pnpm/lockfile-utils@4.0.7
  - @pnpm/config@15.4.1
  - dependency-path@9.2.1
  - @pnpm/read-project-manifest@3.0.6

## 0.5.11

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [c635f9fc1]
- Updated dependencies [47b5e45dd]
  - @pnpm/types@8.3.0
  - dependency-path@9.2.0
  - @pnpm/config@15.4.0
  - @pnpm/lockfile-file@5.1.1
  - @pnpm/lockfile-utils@4.0.6
  - @pnpm/read-project-manifest@3.0.5

## 0.5.10

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [56cf04cb3]
- Updated dependencies [725636a90]
  - @pnpm/types@8.2.0
  - @pnpm/config@15.3.0
  - @pnpm/lockfile-file@5.1.0
  - dependency-path@9.1.4
  - @pnpm/lockfile-utils@4.0.5
  - @pnpm/read-project-manifest@3.0.4

## 0.5.9

### Patch Changes

- Updated dependencies [25798aad1]
  - @pnpm/config@15.2.1

## 0.5.8

### Patch Changes

- Updated dependencies [4d39e4a0c]
- Updated dependencies [bc80631d3]
- Updated dependencies [d5730ba81]
  - @pnpm/types@8.1.0
  - @pnpm/config@15.2.0
  - dependency-path@9.1.3
  - @pnpm/lockfile-file@5.0.4
  - @pnpm/lockfile-utils@4.0.4
  - @pnpm/read-project-manifest@3.0.3

## 0.5.7

### Patch Changes

- Updated dependencies [ae2f845c5]
  - @pnpm/config@15.1.4

## 0.5.6

### Patch Changes

- Updated dependencies [05159665d]
  - @pnpm/config@15.1.3

## 0.5.5

### Patch Changes

- Updated dependencies [af22c6c4f]
- Updated dependencies [c57695550]
  - @pnpm/config@15.1.2
  - dependency-path@9.1.2
  - @pnpm/lockfile-utils@4.0.3

## 0.5.4

### Patch Changes

- Updated dependencies [52b0576af]
  - @pnpm/lockfile-file@5.0.3

## 0.5.3

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/config@15.1.1
  - dependency-path@9.1.1
  - @pnpm/lockfile-file@5.0.2
  - @pnpm/lockfile-utils@4.0.2
  - @pnpm/read-project-manifest@3.0.2

## 0.5.2

### Patch Changes

- Updated dependencies [e05dcc48a]
  - @pnpm/config@15.1.0

## 0.5.1

### Patch Changes

- Updated dependencies [0a70aedb1]
- Updated dependencies [8dac029ef]
- Updated dependencies [688b0eaff]
- Updated dependencies [72b79f55a]
- Updated dependencies [546e644e9]
- Updated dependencies [c6463b9fd]
- Updated dependencies [4bed585e2]
- Updated dependencies [8fa95fd86]
  - dependency-path@9.1.0
  - @pnpm/config@15.0.0
  - @pnpm/lockfile-utils@4.0.1
  - @pnpm/lockfile-file@5.0.1
  - @pnpm/read-project-manifest@3.0.1

## 0.5.0

### Minor Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [516859178]
- Updated dependencies [d504dc380]
- Updated dependencies [73d71a2d5]
- Updated dependencies [fa656992c]
- Updated dependencies [faf830b8f]
- Updated dependencies [542014839]
- Updated dependencies [585e9ca9e]
  - @pnpm/config@14.0.0
  - @pnpm/types@8.0.0
  - dependency-path@9.0.0
  - @pnpm/lockfile-file@5.0.0
  - @pnpm/lockfile-utils@4.0.0
  - @pnpm/read-project-manifest@3.0.0

## 0.4.47

### Patch Changes

- @pnpm/config@13.13.2
- @pnpm/lockfile-file@4.3.1
- @pnpm/read-project-manifest@2.0.13

## 0.4.46

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-file@4.3.0
  - @pnpm/types@7.10.0
  - @pnpm/lockfile-utils@3.2.1
  - @pnpm/config@13.13.1
  - dependency-path@8.0.11
  - @pnpm/read-project-manifest@2.0.12

## 0.4.45

### Patch Changes

- Updated dependencies [334e5340a]
  - @pnpm/config@13.13.0

## 0.4.44

### Patch Changes

- Updated dependencies [b7566b979]
  - @pnpm/config@13.12.0

## 0.4.43

### Patch Changes

- Updated dependencies [cdc521cfa]
  - @pnpm/lockfile-utils@3.2.0
  - @pnpm/config@13.11.0

## 0.4.42

### Patch Changes

- Updated dependencies [fff0e4493]
  - @pnpm/config@13.11.0

## 0.4.41

### Patch Changes

- Updated dependencies [e76151f66]
- Updated dependencies [26cd01b88]
  - @pnpm/config@13.10.0
  - @pnpm/types@7.9.0
  - dependency-path@8.0.10
  - @pnpm/lockfile-file@4.2.6
  - @pnpm/lockfile-utils@3.1.6
  - @pnpm/read-project-manifest@2.0.11

## 0.4.40

### Patch Changes

- Updated dependencies [8fe8f5e55]
  - @pnpm/config@13.9.0

## 0.4.39

### Patch Changes

- Updated dependencies [732d4962f]
- Updated dependencies [a6cf11cb7]
  - @pnpm/config@13.8.0

## 0.4.38

### Patch Changes

- @pnpm/config@13.7.2
- dependency-path@8.0.9
- @pnpm/lockfile-file@4.2.5
- @pnpm/lockfile-utils@3.1.5
- @pnpm/read-project-manifest@2.0.10

## 0.4.37

### Patch Changes

- Updated dependencies [eb9ebd0f3]
- Updated dependencies [eb9ebd0f3]
  - @pnpm/lockfile-file@4.2.4

## 0.4.36

### Patch Changes

- @pnpm/config@13.7.1
- dependency-path@8.0.8
- @pnpm/lockfile-file@4.2.3
- @pnpm/lockfile-utils@3.1.4
- @pnpm/read-project-manifest@2.0.9

## 0.4.35

### Patch Changes

- Updated dependencies [30bfca967]
- Updated dependencies [927c4a089]
- Updated dependencies [10a4bd4db]
  - @pnpm/config@13.7.0
  - dependency-path@8.0.7
  - @pnpm/lockfile-file@4.2.2
  - @pnpm/lockfile-utils@3.1.3
  - @pnpm/read-project-manifest@2.0.8

## 0.4.34

### Patch Changes

- Updated dependencies [46aaf7108]
  - @pnpm/config@13.6.1

## 0.4.33

### Patch Changes

- Updated dependencies [3cf543fc1]
- Updated dependencies [8a99a01ff]
  - @pnpm/lockfile-utils@3.1.2
  - @pnpm/config@13.6.0

## 0.4.32

### Patch Changes

- Updated dependencies [a7ff2d5ce]
  - @pnpm/config@13.5.1

## 0.4.31

### Patch Changes

- Updated dependencies [002778559]
  - @pnpm/config@13.5.0

## 0.4.30

### Patch Changes

- @pnpm/config@13.4.2
- dependency-path@8.0.6
- @pnpm/lockfile-file@4.2.1
- @pnpm/lockfile-utils@3.1.1
- @pnpm/read-project-manifest@2.0.7

## 0.4.29

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/lockfile-file@4.2.0
  - @pnpm/lockfile-utils@3.1.0
  - @pnpm/config@13.4.1
  - dependency-path@8.0.5
  - @pnpm/read-project-manifest@2.0.6

## 0.4.28

### Patch Changes

- Updated dependencies [b6d74c545]
  - @pnpm/config@13.4.0

## 0.4.27

### Patch Changes

- Updated dependencies [bd7bcdbe8]
  - @pnpm/config@13.3.0

## 0.4.26

### Patch Changes

- Updated dependencies [5ee3b2dc7]
  - @pnpm/config@13.2.0

## 0.4.25

### Patch Changes

- Updated dependencies [4027a3c69]
  - @pnpm/config@13.1.0

## 0.4.24

### Patch Changes

- Updated dependencies [fe5688dc0]
- Updated dependencies [c7081cbb4]
- Updated dependencies [c7081cbb4]
  - @pnpm/config@13.0.0

## 0.4.23

### Patch Changes

- Updated dependencies [d62259d67]
  - @pnpm/config@12.6.0

## 0.4.22

### Patch Changes

- Updated dependencies [6681fdcbc]
  - @pnpm/config@12.5.0

## 0.4.21

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9

## 0.4.20

### Patch Changes

- @pnpm/config@12.4.8

## 0.4.19

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7

## 0.4.18

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6

## 0.4.17

### Patch Changes

- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5

## 0.4.16

### Patch Changes

- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4

## 0.4.15

### Patch Changes

- @pnpm/config@12.4.3
- dependency-path@8.0.4
- @pnpm/lockfile-file@4.1.1
- @pnpm/lockfile-utils@3.0.8
- @pnpm/read-project-manifest@2.0.5

## 0.4.14

### Patch Changes

- Updated dependencies [73c1f802e]
  - @pnpm/config@12.4.2

## 0.4.13

### Patch Changes

- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1

## 0.4.12

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0

## 0.4.11

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/lockfile-file@4.1.0
  - @pnpm/config@12.3.3
  - dependency-path@8.0.3
  - @pnpm/lockfile-utils@3.0.7
  - @pnpm/read-project-manifest@2.0.4

## 0.4.10

### Patch Changes

- Updated dependencies [6c418943c]
  - dependency-path@8.0.2
  - @pnpm/lockfile-utils@3.0.6

## 0.4.9

### Patch Changes

- Updated dependencies [2dc5a7a4c]
  - @pnpm/lockfile-file@4.0.4

## 0.4.8

### Patch Changes

- @pnpm/config@12.3.2
- dependency-path@8.0.1
- @pnpm/lockfile-file@4.0.3
- @pnpm/lockfile-utils@3.0.5
- @pnpm/read-project-manifest@2.0.3

## 0.4.7

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1
  - @pnpm/lockfile-file@4.0.2
  - @pnpm/lockfile-utils@3.0.4

## 0.4.6

### Patch Changes

- Updated dependencies [84ec82e05]
- Updated dependencies [c2a71e4fd]
- Updated dependencies [84ec82e05]
  - @pnpm/config@12.3.0

## 0.4.5

### Patch Changes

- Updated dependencies [20e2f235d]
  - dependency-path@8.0.0
  - @pnpm/lockfile-utils@3.0.3

## 0.4.4

### Patch Changes

- @pnpm/read-project-manifest@2.0.2
- @pnpm/config@12.2.0

## 0.4.3

### Patch Changes

- Updated dependencies [05baaa6e7]
- Updated dependencies [dfdf669e6]
- Updated dependencies [6e9c112af]
  - @pnpm/config@12.2.0
  - @pnpm/read-project-manifest@2.0.1
  - dependency-path@7.0.1
  - @pnpm/lockfile-file@4.0.1
  - @pnpm/lockfile-utils@3.0.2

## 0.4.2

### Patch Changes

- Updated dependencies [ba5231ccf]
  - @pnpm/config@12.1.0

## 0.4.1

### Patch Changes

- Updated dependencies [9ceab68f0]
  - dependency-path@7.0.0
  - @pnpm/lockfile-utils@3.0.1

## 0.4.0

### Minor Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.
- 048c94871: `.pnp.js` renamed to `.pnp.cjs` in order to force CommonJS.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [78470a32d]
- Updated dependencies [155e70597]
- Updated dependencies [9c2a878c3]
- Updated dependencies [aed712455]
- Updated dependencies [e4efddbd2]
- Updated dependencies [8b66f26dc]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [f7750baed]
- Updated dependencies [aed712455]
- Updated dependencies [9c2a878c3]
  - @pnpm/config@12.0.0
  - dependency-path@6.0.0
  - @pnpm/lockfile-file@4.0.0
  - @pnpm/lockfile-utils@3.0.0
  - @pnpm/read-project-manifest@2.0.0

## 0.3.25

### Patch Changes

- Updated dependencies [4f1ce907a]
  - @pnpm/config@11.14.2

## 0.3.24

### Patch Changes

- Updated dependencies [4b3852c39]
  - @pnpm/config@11.14.1

## 0.3.23

### Patch Changes

- @pnpm/config@11.14.0

## 0.3.22

### Patch Changes

- Updated dependencies [51e1456dd]
  - @pnpm/lockfile-file@3.2.1

## 0.3.21

### Patch Changes

- Updated dependencies [cb040ae18]
  - @pnpm/config@11.14.0

## 0.3.20

### Patch Changes

- Updated dependencies [ad113645b]
- Updated dependencies [c4cc62506]
  - @pnpm/read-project-manifest@1.1.7
  - @pnpm/config@11.13.0

## 0.3.19

### Patch Changes

- Updated dependencies [bff84dbca]
  - @pnpm/config@11.12.1

## 0.3.18

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [548f28df9]
  - @pnpm/lockfile-file@3.2.0
  - @pnpm/config@11.12.0
  - @pnpm/lockfile-utils@2.0.22
  - dependency-path@5.1.1
  - @pnpm/read-project-manifest@1.1.6

## 0.3.17

### Patch Changes

- @pnpm/config@11.11.1

## 0.3.16

### Patch Changes

- Updated dependencies [af897c324]
  - @pnpm/lockfile-file@3.1.4

## 0.3.15

### Patch Changes

- Updated dependencies [1e4a3a17a]
- Updated dependencies [f40bc5927]
  - @pnpm/lockfile-file@3.1.3
  - @pnpm/config@11.11.0

## 0.3.14

### Patch Changes

- Updated dependencies [e27dcf0dc]
- Updated dependencies [425c7547d]
  - dependency-path@5.1.0
  - @pnpm/config@11.10.2
  - @pnpm/lockfile-utils@2.0.21

## 0.3.13

### Patch Changes

- Updated dependencies [ea09da716]
  - @pnpm/config@11.10.1

## 0.3.12

### Patch Changes

- Updated dependencies [a8656b42f]
  - @pnpm/config@11.10.0

## 0.3.11

### Patch Changes

- Updated dependencies [041537bc3]
  - @pnpm/config@11.9.1

## 0.3.10

### Patch Changes

- Updated dependencies [fba715512]
  - @pnpm/lockfile-file@3.1.2

## 0.3.9

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0
  - @pnpm/lockfile-utils@2.0.20

## 0.3.8

### Patch Changes

- Updated dependencies [fcc1c7100]
  - @pnpm/config@11.8.0

## 0.3.7

### Patch Changes

- @pnpm/config@11.7.2
- @pnpm/lockfile-file@3.1.1
- @pnpm/read-project-manifest@1.1.5

## 0.3.6

### Patch Changes

- Updated dependencies [3776b5a52]
  - @pnpm/lockfile-file@3.1.0

## 0.3.5

### Patch Changes

- Updated dependencies [dbcc6c96f]
- Updated dependencies [09492b7b4]
  - @pnpm/lockfile-file@3.0.18
  - @pnpm/read-project-manifest@1.1.4

## 0.3.4

### Patch Changes

- 60e01bd1d: @pnpm/logger should not be a prod dependency because it is a peer dependency.
- Updated dependencies [39142e2ad]
- Updated dependencies [aa6bc4f95]
  - dependency-path@5.0.6
  - @pnpm/lockfile-file@3.0.17
  - @pnpm/lockfile-utils@2.0.19
  - @pnpm/read-project-manifest@1.1.3

## 0.3.3

### Patch Changes

- @pnpm/lockfile-file@3.0.16
- @pnpm/lockfile-utils@2.0.18
- @pnpm/config@11.7.1
- dependency-path@5.0.5
- @pnpm/read-project-manifest@1.1.2

## 0.3.2

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0

## 0.3.1

### Patch Changes

- @pnpm/lockfile-file@3.0.15
- @pnpm/lockfile-utils@2.0.17
- @pnpm/config@11.6.1
- dependency-path@5.0.4
- @pnpm/read-project-manifest@1.1.1

## 0.3.0

### Minor Changes

- f591fdeeb: CLI command removed.

### Patch Changes

- Updated dependencies [f591fdeeb]
  - @pnpm/config@11.6.0

## 0.2.0

### Minor Changes

- faac0745b: Rename lockfileDirectory to lockfileDir.

### Patch Changes

- faac0745b: Always set the packageLocation correctly.
- Updated dependencies [2762781cc]
  - @pnpm/read-project-manifest@1.1.0

## 0.1.4

### Patch Changes

- 119da15e9: pathLocation of workspace project should always start with "./"

## 0.1.3

### Patch Changes

- 646c7868b: `@pnpm/logger` should be a prod dependency as lockfile-to-pnp is a standalone CLI app.

## 0.1.2

### Patch Changes

- c3d34232c: Normalize `packageLocation` path.
- c3d34232c: Use correct return type for `lockfileToPackageRegistry`.
- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0

## 0.1.1

### Patch Changes

- ee91574b7: packageLocation should be a relative path.

## 0.1.0

### Minor Changes

- c9f0c7764: Initial version.

### Patch Changes

- Updated dependencies [23cf3c88b]
  - @pnpm/config@11.4.0
