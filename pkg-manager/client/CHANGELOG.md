# @pnpm/client

## 11.1.3

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/resolver-base@13.0.0
  - @pnpm/types@11.0.0
  - @pnpm/directory-fetcher@8.0.4
  - @pnpm/default-resolver@20.0.2
  - @pnpm/git-fetcher@13.0.7
  - @pnpm/tarball-fetcher@19.0.7
  - @pnpm/fetch@8.0.3

## 11.1.2

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/directory-fetcher@8.0.3
  - @pnpm/git-fetcher@13.0.6
  - @pnpm/tarball-fetcher@19.0.6
  - @pnpm/fetch@8.0.2
  - @pnpm/resolver-base@12.0.2
  - @pnpm/default-resolver@20.0.1

## 11.1.1

### Patch Changes

- @pnpm/git-fetcher@13.0.5
- @pnpm/tarball-fetcher@19.0.5

## 11.1.0

### Minor Changes

- 0c08e1c: Added a new function for clearing resolution cache.

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/default-resolver@20.0.0
  - @pnpm/git-fetcher@13.0.4
  - @pnpm/tarball-fetcher@19.0.4
  - @pnpm/directory-fetcher@8.0.2

## 11.0.6

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/directory-fetcher@8.0.2
  - @pnpm/git-fetcher@13.0.3
  - @pnpm/tarball-fetcher@19.0.3
  - @pnpm/fetch@8.0.1
  - @pnpm/resolver-base@12.0.1
  - @pnpm/default-resolver@19.0.5

## 11.0.5

### Patch Changes

- @pnpm/tarball-fetcher@19.0.2
- @pnpm/network.auth-header@3.0.1
- @pnpm/default-resolver@19.0.4
- @pnpm/git-fetcher@13.0.2
- @pnpm/directory-fetcher@8.0.1

## 11.0.4

### Patch Changes

- @pnpm/default-resolver@19.0.3

## 11.0.3

### Patch Changes

- @pnpm/default-resolver@19.0.2

## 11.0.2

### Patch Changes

- @pnpm/default-resolver@19.0.1

## 11.0.1

### Patch Changes

- @pnpm/git-fetcher@13.0.1
- @pnpm/tarball-fetcher@19.0.1

## 11.0.0

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
- Updated dependencies [36dcaa0]
- Updated dependencies [b13d2dc]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/fetch@8.0.0
  - @pnpm/tarball-fetcher@19.0.0
  - @pnpm/directory-fetcher@8.0.0
  - @pnpm/default-resolver@19.0.0
  - @pnpm/resolver-base@12.0.0
  - @pnpm/fetching-types@6.0.0
  - @pnpm/git-fetcher@13.0.0
  - @pnpm/network.auth-header@3.0.0

## 10.0.46

### Patch Changes

- Updated dependencies [31054a63e]
  - @pnpm/resolver-base@11.1.0
  - @pnpm/directory-fetcher@7.0.11
  - @pnpm/default-resolver@18.0.22
  - @pnpm/git-fetcher@12.0.19
  - @pnpm/tarball-fetcher@18.0.19

## 10.0.45

### Patch Changes

- Updated dependencies [342222d20]
  - @pnpm/tarball-fetcher@18.0.18
  - @pnpm/directory-fetcher@7.0.10
  - @pnpm/git-fetcher@12.0.18

## 10.0.44

### Patch Changes

- Updated dependencies [5a5e42551]
  - @pnpm/network.auth-header@2.2.0
  - @pnpm/directory-fetcher@7.0.9
  - @pnpm/tarball-fetcher@18.0.17

## 10.0.43

### Patch Changes

- @pnpm/git-fetcher@12.0.17
- @pnpm/tarball-fetcher@18.0.17

## 10.0.42

### Patch Changes

- @pnpm/default-resolver@18.0.21
- @pnpm/git-fetcher@12.0.16
- @pnpm/directory-fetcher@7.0.9
- @pnpm/tarball-fetcher@18.0.16
- @pnpm/resolver-base@11.0.2
- @pnpm/fetch@7.0.7

## 10.0.41

### Patch Changes

- @pnpm/directory-fetcher@7.0.8
- @pnpm/git-fetcher@12.0.15
- @pnpm/tarball-fetcher@18.0.15
- @pnpm/resolver-base@11.0.1
- @pnpm/fetch@7.0.6
- @pnpm/default-resolver@18.0.20

## 10.0.40

### Patch Changes

- @pnpm/git-fetcher@12.0.14
- @pnpm/tarball-fetcher@18.0.14

## 10.0.39

### Patch Changes

- Updated dependencies [3ac0487b3]
  - @pnpm/network.auth-header@2.1.0

## 10.0.38

### Patch Changes

- Updated dependencies [23039a6d6]
  - @pnpm/network.auth-header@2.0.6

## 10.0.37

### Patch Changes

- @pnpm/default-resolver@18.0.19

## 10.0.36

### Patch Changes

- @pnpm/git-fetcher@12.0.13
- @pnpm/tarball-fetcher@18.0.13

## 10.0.35

### Patch Changes

- Updated dependencies [abdf1f2b6]
  - @pnpm/tarball-fetcher@18.0.12
  - @pnpm/directory-fetcher@7.0.7
  - @pnpm/git-fetcher@12.0.12

## 10.0.34

### Patch Changes

- @pnpm/directory-fetcher@7.0.6
- @pnpm/git-fetcher@12.0.11
- @pnpm/tarball-fetcher@18.0.11

## 10.0.33

### Patch Changes

- Updated dependencies [4c2450208]
  - @pnpm/resolver-base@11.0.0
  - @pnpm/directory-fetcher@7.0.5
  - @pnpm/default-resolver@18.0.18
  - @pnpm/git-fetcher@12.0.10
  - @pnpm/tarball-fetcher@18.0.10

## 10.0.32

### Patch Changes

- Updated dependencies [500363647]
  - @pnpm/directory-fetcher@7.0.4
  - @pnpm/git-fetcher@12.0.9
  - @pnpm/tarball-fetcher@18.0.9

## 10.0.31

### Patch Changes

- @pnpm/directory-fetcher@7.0.3
- @pnpm/git-fetcher@12.0.8
- @pnpm/tarball-fetcher@18.0.8
- @pnpm/resolver-base@10.0.4
- @pnpm/default-resolver@18.0.17
- @pnpm/fetch@7.0.5

## 10.0.30

### Patch Changes

- @pnpm/git-fetcher@12.0.7
- @pnpm/tarball-fetcher@18.0.7

## 10.0.29

### Patch Changes

- @pnpm/git-fetcher@12.0.6
- @pnpm/tarball-fetcher@18.0.6

## 10.0.28

### Patch Changes

- @pnpm/default-resolver@18.0.16
- @pnpm/git-fetcher@12.0.5
- @pnpm/tarball-fetcher@18.0.5
- @pnpm/directory-fetcher@7.0.2

## 10.0.27

### Patch Changes

- @pnpm/git-fetcher@12.0.4
- @pnpm/tarball-fetcher@18.0.4

## 10.0.26

### Patch Changes

- @pnpm/git-fetcher@12.0.3
- @pnpm/tarball-fetcher@18.0.3

## 10.0.25

### Patch Changes

- @pnpm/directory-fetcher@7.0.2
- @pnpm/git-fetcher@12.0.2
- @pnpm/tarball-fetcher@18.0.2
- @pnpm/resolver-base@10.0.3
- @pnpm/fetch@7.0.4
- @pnpm/default-resolver@18.0.15

## 10.0.24

### Patch Changes

- @pnpm/git-fetcher@12.0.1
- @pnpm/tarball-fetcher@18.0.1

## 10.0.23

### Patch Changes

- Updated dependencies [9caa33d53]
  - @pnpm/tarball-fetcher@18.0.0
  - @pnpm/git-fetcher@12.0.0
  - @pnpm/directory-fetcher@7.0.1
  - @pnpm/default-resolver@18.0.14

## 10.0.22

### Patch Changes

- @pnpm/git-fetcher@11.0.1
- @pnpm/tarball-fetcher@17.0.1

## 10.0.21

### Patch Changes

- @pnpm/git-fetcher@11.0.0
- @pnpm/tarball-fetcher@17.0.0
- @pnpm/directory-fetcher@7.0.0

## 10.0.20

### Patch Changes

- @pnpm/git-fetcher@10.0.2
- @pnpm/tarball-fetcher@16.0.2
- @pnpm/directory-fetcher@7.0.0

## 10.0.19

### Patch Changes

- Updated dependencies [4a1a9431d]
- Updated dependencies [d92070876]
  - @pnpm/directory-fetcher@7.0.0
  - @pnpm/git-fetcher@10.0.1
  - @pnpm/tarball-fetcher@16.0.1

## 10.0.18

### Patch Changes

- Updated dependencies [d57e4de6d]
- Updated dependencies [083bbf590]
- Updated dependencies [70b2830ac]
- Updated dependencies [96e165c7f]
  - @pnpm/directory-fetcher@6.1.0
  - @pnpm/tarball-fetcher@16.0.0
  - @pnpm/git-fetcher@10.0.0
  - @pnpm/default-resolver@18.0.13

## 10.0.17

### Patch Changes

- Updated dependencies [840b65bda]
  - @pnpm/tarball-fetcher@15.0.9

## 10.0.16

### Patch Changes

- Updated dependencies [aa20818a0]
  - @pnpm/network.auth-header@2.0.5
  - @pnpm/git-fetcher@9.0.7
  - @pnpm/tarball-fetcher@15.0.8

## 10.0.15

### Patch Changes

- @pnpm/git-fetcher@9.0.6
- @pnpm/tarball-fetcher@15.0.7
- @pnpm/default-resolver@18.0.12
- @pnpm/directory-fetcher@6.0.4

## 10.0.14

### Patch Changes

- @pnpm/default-resolver@18.0.11

## 10.0.13

### Patch Changes

- @pnpm/git-fetcher@9.0.5
- @pnpm/tarball-fetcher@15.0.6
- @pnpm/resolver-base@10.0.2
- @pnpm/default-resolver@18.0.10
- @pnpm/directory-fetcher@6.0.4
- @pnpm/fetch@7.0.3

## 10.0.12

### Patch Changes

- @pnpm/directory-fetcher@6.0.3
- @pnpm/default-resolver@18.0.9
- @pnpm/git-fetcher@9.0.4
- @pnpm/tarball-fetcher@15.0.5

## 10.0.11

### Patch Changes

- @pnpm/git-fetcher@9.0.3
- @pnpm/tarball-fetcher@15.0.4
- @pnpm/directory-fetcher@6.0.2

## 10.0.10

### Patch Changes

- Updated dependencies [e44031e71]
  - @pnpm/network.auth-header@2.0.4

## 10.0.9

### Patch Changes

- Updated dependencies [4e7afec90]
  - @pnpm/network.auth-header@2.0.3

## 10.0.8

### Patch Changes

- @pnpm/tarball-fetcher@15.0.3
- @pnpm/network.auth-header@2.0.2
- @pnpm/default-resolver@18.0.8
- @pnpm/directory-fetcher@6.0.2
- @pnpm/git-fetcher@9.0.2

## 10.0.7

### Patch Changes

- Updated dependencies [d55b41a8b]
  - @pnpm/tarball-fetcher@15.0.2
  - @pnpm/default-resolver@18.0.7
  - @pnpm/git-fetcher@9.0.1
  - @pnpm/directory-fetcher@6.0.1

## 10.0.6

### Patch Changes

- @pnpm/default-resolver@18.0.6
- @pnpm/git-fetcher@9.0.1
- @pnpm/resolver-base@10.0.1
- @pnpm/tarball-fetcher@15.0.1
- @pnpm/directory-fetcher@6.0.1
- @pnpm/fetch@7.0.2
- @pnpm/network.auth-header@2.0.1

## 10.0.5

### Patch Changes

- @pnpm/default-resolver@18.0.5
- @pnpm/directory-fetcher@6.0.0
- @pnpm/tarball-fetcher@15.0.0

## 10.0.4

### Patch Changes

- Updated dependencies [8228c2cb1]
  - @pnpm/fetch@7.0.1
  - @pnpm/tarball-fetcher@15.0.0
  - @pnpm/default-resolver@18.0.4

## 10.0.3

### Patch Changes

- @pnpm/default-resolver@18.0.3

## 10.0.2

### Patch Changes

- @pnpm/default-resolver@18.0.2

## 10.0.1

### Patch Changes

- @pnpm/default-resolver@18.0.1

## 10.0.0

### Major Changes

- 7a0ce1df0: When there's a `files` field in the `package.json`, only deploy those files that are listed in it.
  Use the same logic also when injecting packages. This behavior can be changed by setting the `deploy-all-files` setting to `true` [#5911](https://github.com/pnpm/pnpm/issues/5911).
- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/directory-fetcher@6.0.0
  - @pnpm/default-resolver@18.0.0
  - @pnpm/tarball-fetcher@15.0.0
  - @pnpm/resolver-base@10.0.0
  - @pnpm/fetching-types@5.0.0
  - @pnpm/git-fetcher@9.0.0
  - @pnpm/network.auth-header@2.0.0
  - @pnpm/fetch@7.0.0

## 9.1.5

### Patch Changes

- @pnpm/tarball-fetcher@14.1.4
- @pnpm/directory-fetcher@5.1.6
- @pnpm/default-resolver@17.0.11
- @pnpm/git-fetcher@8.0.2

## 9.1.4

### Patch Changes

- Updated dependencies [2241f77ad]
- Updated dependencies [673e23060]
- Updated dependencies [9fa6c7404]
  - @pnpm/tarball-fetcher@14.1.3
  - @pnpm/fetch@6.0.6
  - @pnpm/default-resolver@17.0.10

## 9.1.3

### Patch Changes

- Updated dependencies [029143cff]
- Updated dependencies [029143cff]
  - @pnpm/resolver-base@9.2.0
  - @pnpm/directory-fetcher@5.1.5
  - @pnpm/default-resolver@17.0.9
  - @pnpm/git-fetcher@8.0.1
  - @pnpm/tarball-fetcher@14.1.2

## 9.1.2

### Patch Changes

- @pnpm/default-resolver@17.0.8

## 9.1.1

### Patch Changes

- Updated dependencies [1e6de89b6]
  - @pnpm/tarball-fetcher@14.1.1
  - @pnpm/default-resolver@17.0.7
  - @pnpm/git-fetcher@8.0.0
  - @pnpm/directory-fetcher@5.1.4

## 9.1.0

### Minor Changes

- c7b05cd9a: When ignoreScripts=true is passed to the fetcher, do not build git-hosted dependencies.

### Patch Changes

- Updated dependencies [c7b05cd9a]
- Updated dependencies [c7b05cd9a]
  - @pnpm/tarball-fetcher@14.1.0
  - @pnpm/git-fetcher@8.0.0
  - @pnpm/network.auth-header@1.0.1
  - @pnpm/default-resolver@17.0.6
  - @pnpm/directory-fetcher@5.1.4

## 9.0.1

### Patch Changes

- ec97a3105: Report to the console when a git-hosted dependency is built [#5847](https://github.com/pnpm/pnpm/pull/5847).
- Updated dependencies [ec97a3105]
  - @pnpm/tarball-fetcher@14.0.1
  - @pnpm/git-fetcher@7.0.1
  - @pnpm/default-resolver@17.0.5

## 9.0.0

### Major Changes

- 339c0a704: A new required option added to the prepare package function: rawConfig. It is needed in order to create a proper environment for the package manager executed during the preparation of a git-hosted dependency.

### Patch Changes

- Updated dependencies [339c0a704]
  - @pnpm/git-fetcher@7.0.0
  - @pnpm/tarball-fetcher@14.0.0

## 8.1.3

### Patch Changes

- @pnpm/git-fetcher@6.0.4
- @pnpm/resolver-base@9.1.5
- @pnpm/tarball-fetcher@13.0.3
- @pnpm/directory-fetcher@5.1.3
- @pnpm/fetch@6.0.5
- @pnpm/default-resolver@17.0.4

## 8.1.2

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/tarball-fetcher@13.0.2
  - @pnpm/fetch@6.0.4
  - @pnpm/default-resolver@17.0.3
  - @pnpm/git-fetcher@6.0.3
  - @pnpm/directory-fetcher@5.1.2

## 8.1.1

### Patch Changes

- @pnpm/directory-fetcher@5.1.1
- @pnpm/default-resolver@17.0.2

## 8.1.0

### Minor Changes

- eacff33e4: New option added to resolve symlinks to their real locations, when injecting directories.

### Patch Changes

- Updated dependencies [eacff33e4]
  - @pnpm/directory-fetcher@5.1.0

## 8.0.3

### Patch Changes

- Updated dependencies [6710d9dd9]
- Updated dependencies [6710d9dd9]
  - @pnpm/directory-fetcher@5.0.0
  - @pnpm/git-fetcher@6.0.2
  - @pnpm/resolver-base@9.1.4
  - @pnpm/fetch@6.0.3
  - @pnpm/tarball-fetcher@13.0.1
  - @pnpm/default-resolver@17.0.1

## 8.0.2

### Patch Changes

- Updated dependencies [804de211e]
- Updated dependencies [804de211e]
  - @pnpm/default-resolver@17.0.0
  - @pnpm/fetching-types@4.0.0
  - @pnpm/tarball-fetcher@13.0.0
  - @pnpm/network.auth-header@1.0.0
  - @pnpm/fetch@6.0.2

## 8.0.1

### Patch Changes

- @pnpm/git-fetcher@6.0.1
- @pnpm/resolver-base@9.1.3
- @pnpm/fetch@6.0.1
- @pnpm/tarball-fetcher@12.0.1
- @pnpm/directory-fetcher@4.0.1
- @pnpm/default-resolver@16.0.1

## 8.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/default-resolver@16.0.0
  - @pnpm/directory-fetcher@4.0.0
  - @pnpm/fetch@6.0.0
  - @pnpm/git-fetcher@6.0.0
  - @pnpm/tarball-fetcher@12.0.0

## 7.2.10

### Patch Changes

- @pnpm/directory-fetcher@3.1.5
- @pnpm/default-resolver@15.0.24

## 7.2.9

### Patch Changes

- @pnpm/fetch@5.0.10
- @pnpm/tarball-fetcher@11.0.5
- @pnpm/default-resolver@15.0.23
- @pnpm/git-fetcher@5.2.4

## 7.2.8

### Patch Changes

- @pnpm/default-resolver@15.0.22
- @pnpm/tarball-fetcher@11.0.4
- @pnpm/git-fetcher@5.2.4
- @pnpm/directory-fetcher@3.1.4

## 7.2.7

### Patch Changes

- @pnpm/git-fetcher@5.2.3
- @pnpm/resolver-base@9.1.2
- @pnpm/fetch@5.0.9
- @pnpm/tarball-fetcher@11.0.3
- @pnpm/directory-fetcher@3.1.3
- @pnpm/default-resolver@15.0.21

## 7.2.6

### Patch Changes

- @pnpm/git-fetcher@5.2.2
- @pnpm/resolver-base@9.1.1
- @pnpm/fetch@5.0.8
- @pnpm/tarball-fetcher@11.0.2
- @pnpm/directory-fetcher@3.1.2
- @pnpm/default-resolver@15.0.20

## 7.2.5

### Patch Changes

- @pnpm/default-resolver@15.0.19

## 7.2.4

### Patch Changes

- @pnpm/default-resolver@15.0.18

## 7.2.3

### Patch Changes

- @pnpm/default-resolver@15.0.17

## 7.2.2

### Patch Changes

- @pnpm/default-resolver@15.0.16

## 7.2.1

### Patch Changes

- Updated dependencies [dbac0ca01]
- Updated dependencies [07bc24ad1]
  - @pnpm/tarball-fetcher@11.0.1
  - @pnpm/directory-fetcher@3.1.1
  - @pnpm/git-fetcher@5.2.1
  - @pnpm/default-resolver@15.0.15

## 7.2.0

### Minor Changes

- 23984abd1: Add hook for adding custom fetchers.

### Patch Changes

- Updated dependencies [32915f0e4]
- Updated dependencies [23984abd1]
- Updated dependencies [7a17f99ab]
  - @pnpm/tarball-fetcher@11.0.0
  - @pnpm/directory-fetcher@3.1.0
  - @pnpm/git-fetcher@5.2.0
  - @pnpm/resolver-base@9.1.0
  - @pnpm/default-resolver@15.0.14

## 7.1.14

### Patch Changes

- @pnpm/default-resolver@15.0.13
- @pnpm/git-fetcher@5.1.7
- @pnpm/tarball-fetcher@10.0.10
- @pnpm/directory-fetcher@3.0.10

## 7.1.13

### Patch Changes

- Updated dependencies [39c040127]
- Updated dependencies [8103f92bd]
  - @pnpm/directory-fetcher@3.0.10
  - @pnpm/tarball-fetcher@10.0.10
  - @pnpm/git-fetcher@5.1.7
  - @pnpm/default-resolver@15.0.12

## 7.1.12

### Patch Changes

- @pnpm/default-resolver@15.0.11
- @pnpm/git-fetcher@5.1.7
- @pnpm/resolver-base@9.0.6
- @pnpm/fetch@5.0.7
- @pnpm/tarball-fetcher@10.0.9
- @pnpm/directory-fetcher@3.0.9

## 7.1.11

### Patch Changes

- @pnpm/directory-fetcher@3.0.8
- @pnpm/default-resolver@15.0.10

## 7.1.10

### Patch Changes

- Updated dependencies [e018a8b14]
  - @pnpm/fetch@5.0.6
  - @pnpm/default-resolver@15.0.9
  - @pnpm/tarball-fetcher@10.0.8

## 7.1.9

### Patch Changes

- @pnpm/default-resolver@15.0.8

## 7.1.8

### Patch Changes

- Updated dependencies [5f643f23b]
  - @pnpm/directory-fetcher@3.0.7
  - @pnpm/tarball-fetcher@10.0.8

## 7.1.7

### Patch Changes

- @pnpm/git-fetcher@5.1.6
- @pnpm/resolver-base@9.0.5
- @pnpm/fetch@5.0.5
- @pnpm/tarball-fetcher@10.0.7
- @pnpm/directory-fetcher@3.0.6
- @pnpm/default-resolver@15.0.7

## 7.1.6

### Patch Changes

- @pnpm/git-fetcher@5.1.5
- @pnpm/resolver-base@9.0.4
- @pnpm/tarball-fetcher@10.0.6
- @pnpm/directory-fetcher@3.0.5
- @pnpm/fetch@5.0.4
- @pnpm/default-resolver@15.0.6

## 7.1.5

### Patch Changes

- Updated dependencies [9d5bf09c0]
  - @pnpm/fetch@5.0.3
  - @pnpm/git-fetcher@5.1.4
  - @pnpm/resolver-base@9.0.3
  - @pnpm/default-resolver@15.0.5
  - @pnpm/tarball-fetcher@10.0.5
  - @pnpm/directory-fetcher@3.0.4

## 7.1.4

### Patch Changes

- @pnpm/git-fetcher@5.1.3
- @pnpm/resolver-base@9.0.2
- @pnpm/fetch@5.0.2
- @pnpm/tarball-fetcher@10.0.4
- @pnpm/directory-fetcher@3.0.3
- @pnpm/default-resolver@15.0.4

## 7.1.3

### Patch Changes

- @pnpm/git-fetcher@5.1.2
- @pnpm/directory-fetcher@3.0.2
- @pnpm/tarball-fetcher@10.0.3

## 7.1.2

### Patch Changes

- @pnpm/default-resolver@15.0.3

## 7.1.1

### Patch Changes

- @pnpm/git-fetcher@5.1.1
- @pnpm/tarball-fetcher@10.0.2
- @pnpm/resolver-base@9.0.1
- @pnpm/fetch@5.0.1
- @pnpm/directory-fetcher@3.0.1
- @pnpm/default-resolver@15.0.2

## 7.1.0

### Minor Changes

- c6463b9fd: New setting added: `git-shallow-hosts`. When cloning repositories from "shallow-hosts", pnpm will use shallow cloning to fetch only the needed commit, not all the history [#4548](https://github.com/pnpm/pnpm/pull/4548).

### Patch Changes

- Updated dependencies [c6463b9fd]
- Updated dependencies [41cae6450]
  - @pnpm/git-fetcher@5.1.0
  - @pnpm/directory-fetcher@3.0.0
  - @pnpm/tarball-fetcher@10.0.1
  - @pnpm/default-resolver@15.0.1

## 7.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/default-resolver@15.0.0
  - @pnpm/directory-fetcher@2.0.0
  - @pnpm/fetch@5.0.0
  - @pnpm/fetching-types@3.0.0
  - @pnpm/git-fetcher@5.0.0
  - @pnpm/resolver-base@9.0.0
  - @pnpm/tarball-fetcher@10.0.0

## 6.1.3

### Patch Changes

- @pnpm/default-resolver@14.0.12
- @pnpm/tarball-fetcher@9.3.17
- @pnpm/git-fetcher@4.1.16
- @pnpm/directory-fetcher@1.0.7

## 6.1.2

### Patch Changes

- @pnpm/git-fetcher@4.1.15
- @pnpm/resolver-base@8.1.6
- @pnpm/tarball-fetcher@9.3.16
- @pnpm/fetch@4.2.5
- @pnpm/directory-fetcher@1.0.6
- @pnpm/default-resolver@14.0.11

## 6.1.1

### Patch Changes

- Updated dependencies [aa1f9dc19]
- Updated dependencies [4f78a2a5f]
  - @pnpm/directory-fetcher@1.0.5
  - @pnpm/git-fetcher@4.1.14
  - @pnpm/resolver-base@8.1.5
  - @pnpm/fetch@4.2.4
  - @pnpm/tarball-fetcher@9.3.15
  - @pnpm/default-resolver@14.0.10

## 6.1.0

### Minor Changes

- a6cf11cb7: New optional setting added: userConfig. userConfig may contain token helpers.

## 6.0.11

### Patch Changes

- @pnpm/git-fetcher@4.1.13
- @pnpm/resolver-base@8.1.4
- @pnpm/fetch@4.2.3
- @pnpm/tarball-fetcher@9.3.14
- @pnpm/directory-fetcher@1.0.4
- @pnpm/default-resolver@14.0.9

## 6.0.10

### Patch Changes

- @pnpm/git-fetcher@4.1.12
- @pnpm/resolver-base@8.1.3
- @pnpm/tarball-fetcher@9.3.13
- @pnpm/fetch@4.2.2
- @pnpm/directory-fetcher@1.0.3
- @pnpm/default-resolver@14.0.8

## 6.0.9

### Patch Changes

- @pnpm/fetch@4.2.1
- @pnpm/tarball-fetcher@9.3.12
- @pnpm/git-fetcher@4.1.11
- @pnpm/resolver-base@8.1.2
- @pnpm/default-resolver@14.0.7
- @pnpm/directory-fetcher@1.0.2

## 6.0.8

### Patch Changes

- Updated dependencies [b13e4b452]
  - @pnpm/git-fetcher@4.1.10
  - @pnpm/tarball-fetcher@9.3.11

## 6.0.7

### Patch Changes

- Updated dependencies [f1c194ded]
  - @pnpm/fetch@4.2.0
  - @pnpm/default-resolver@14.0.6
  - @pnpm/tarball-fetcher@9.3.10

## 6.0.6

### Patch Changes

- Updated dependencies [fb1a95a6c]
- Updated dependencies [fb1a95a6c]
  - @pnpm/git-fetcher@4.1.9
  - @pnpm/tarball-fetcher@9.3.10

## 6.0.5

### Patch Changes

- Updated dependencies [12ee3c144]
  - @pnpm/fetch@4.1.6
  - @pnpm/default-resolver@14.0.5
  - @pnpm/tarball-fetcher@9.3.9

## 6.0.4

### Patch Changes

- @pnpm/default-resolver@14.0.4

## 6.0.3

### Patch Changes

- @pnpm/default-resolver@14.0.3

## 6.0.2

### Patch Changes

- Updated dependencies [108bd4a39]
  - @pnpm/directory-fetcher@1.0.1
  - @pnpm/git-fetcher@4.1.8
  - @pnpm/resolver-base@8.1.1
  - @pnpm/default-resolver@14.0.2
  - @pnpm/fetch@4.1.5
  - @pnpm/tarball-fetcher@9.3.9

## 6.0.1

### Patch Changes

- @pnpm/default-resolver@14.0.1

## 6.0.0

### Major Changes

- 4ab87844a: Local directory dependencies are resolved to absolute path.

### Minor Changes

- 4ab87844a: New fetcher added for fetching local directory dependencies.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/resolver-base@8.1.0
  - @pnpm/directory-fetcher@1.0.0
  - @pnpm/default-resolver@14.0.0
  - @pnpm/git-fetcher@4.1.7
  - @pnpm/tarball-fetcher@9.3.8
  - @pnpm/fetch@4.1.4

## 5.0.10

### Patch Changes

- Updated dependencies [782ef2490]
  - @pnpm/fetch@4.1.3
  - @pnpm/default-resolver@13.0.9
  - @pnpm/tarball-fetcher@9.3.7

## 5.0.9

### Patch Changes

- @pnpm/default-resolver@13.0.8

## 5.0.8

### Patch Changes

- @pnpm/fetch@4.1.2
- @pnpm/default-resolver@13.0.7
- @pnpm/tarball-fetcher@9.3.7

## 5.0.7

### Patch Changes

- @pnpm/git-fetcher@4.1.6
- @pnpm/tarball-fetcher@9.3.7

## 5.0.6

### Patch Changes

- Updated dependencies [04b7f6086]
  - @pnpm/git-fetcher@4.1.5
  - @pnpm/default-resolver@13.0.6

## 5.0.5

### Patch Changes

- Updated dependencies [bab172385]
  - @pnpm/fetch@4.1.1
  - @pnpm/fetching-types@2.2.1
  - @pnpm/default-resolver@13.0.5
  - @pnpm/tarball-fetcher@9.3.6
  - @pnpm/git-fetcher@4.1.4

## 5.0.4

### Patch Changes

- Updated dependencies [eadf0e505]
  - @pnpm/fetch@4.1.0
  - @pnpm/fetching-types@2.2.0
  - @pnpm/default-resolver@13.0.4
  - @pnpm/tarball-fetcher@9.3.5

## 5.0.3

### Patch Changes

- @pnpm/default-resolver@13.0.3

## 5.0.2

### Patch Changes

- @pnpm/default-resolver@13.0.2

## 5.0.1

### Patch Changes

- @pnpm/git-fetcher@4.1.4
- @pnpm/resolver-base@8.0.4
- @pnpm/fetch@4.0.2
- @pnpm/tarball-fetcher@9.3.4
- @pnpm/default-resolver@13.0.1

## 5.0.0

### Major Changes

- 691f64713: New required option added: cacheDir.

### Minor Changes

- 691f64713: New config setting added: cacheDir.

### Patch Changes

- Updated dependencies [691f64713]
  - @pnpm/default-resolver@13.0.0

## 4.0.2

### Patch Changes

- @pnpm/git-fetcher@4.1.3
- @pnpm/resolver-base@8.0.3
- @pnpm/fetch@4.0.1
- @pnpm/tarball-fetcher@9.3.3
- @pnpm/default-resolver@12.0.7

## 4.0.1

### Patch Changes

- @pnpm/default-resolver@12.0.6

## 4.0.0

### Major Changes

- eeff424bd: strictSSL option renamed to strictSsl.

### Patch Changes

- Updated dependencies [e7d9cd187]
- Updated dependencies [eeff424bd]
  - @pnpm/fetch@4.0.0
  - @pnpm/default-resolver@12.0.5
  - @pnpm/tarball-fetcher@9.3.2
  - @pnpm/git-fetcher@4.1.2
  - @pnpm/resolver-base@8.0.2

## 3.1.6

### Patch Changes

- Updated dependencies [a1a03d145]
  - @pnpm/tarball-fetcher@9.3.1
  - @pnpm/git-fetcher@4.1.1

## 3.1.5

### Patch Changes

- Updated dependencies [6d2ccc9a3]
  - @pnpm/tarball-fetcher@9.3.0

## 3.1.4

### Patch Changes

- @pnpm/default-resolver@12.0.4
- @pnpm/git-fetcher@4.1.1
- @pnpm/tarball-fetcher@9.2.2

## 3.1.3

### Patch Changes

- @pnpm/tarball-fetcher@9.2.2
- @pnpm/git-fetcher@4.1.1
- @pnpm/default-resolver@12.0.3

## 3.1.2

### Patch Changes

- Updated dependencies [3b147ced9]
  - @pnpm/git-fetcher@4.1.1
  - @pnpm/tarball-fetcher@9.2.1

## 3.1.1

### Patch Changes

- Updated dependencies [e6a2654a2]
  - @pnpm/git-fetcher@4.1.0
  - @pnpm/tarball-fetcher@9.2.0

## 3.1.0

### Minor Changes

- 05baaa6e7: Add new option: timeout.

### Patch Changes

- Updated dependencies [05baaa6e7]
  - @pnpm/fetch@3.1.0
  - @pnpm/fetching-types@2.1.0
  - @pnpm/tarball-fetcher@9.1.0
  - @pnpm/default-resolver@12.0.2
  - @pnpm/git-fetcher@4.0.1
  - @pnpm/resolver-base@8.0.1

## 3.0.1

### Patch Changes

- @pnpm/git-fetcher@4.0.0
- @pnpm/tarball-fetcher@9.0.0
- @pnpm/default-resolver@12.0.1

## 3.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [83645c8ed]
  - @pnpm/default-resolver@12.0.0
  - @pnpm/fetch@3.0.0
  - @pnpm/fetching-types@2.0.0
  - @pnpm/git-fetcher@4.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/tarball-fetcher@9.0.0

## 2.0.24

### Patch Changes

- @pnpm/fetch@2.1.11
- @pnpm/default-resolver@11.0.20
- @pnpm/tarball-fetcher@8.2.8

## 2.0.23

### Patch Changes

- @pnpm/git-fetcher@3.0.13
- @pnpm/tarball-fetcher@8.2.8

## 2.0.22

### Patch Changes

- @pnpm/default-resolver@11.0.19

## 2.0.21

### Patch Changes

- Updated dependencies [ad113645b]
  - @pnpm/tarball-fetcher@8.2.8
  - @pnpm/default-resolver@11.0.18

## 2.0.20

### Patch Changes

- @pnpm/git-fetcher@3.0.13
- @pnpm/resolver-base@7.1.1
- @pnpm/fetch@2.1.10
- @pnpm/tarball-fetcher@8.2.7
- @pnpm/default-resolver@11.0.17

## 2.0.19

### Patch Changes

- Updated dependencies [32c9ef4be]
  - @pnpm/git-fetcher@3.0.12
  - @pnpm/default-resolver@11.0.16

## 2.0.18

### Patch Changes

- @pnpm/default-resolver@11.0.15

## 2.0.17

### Patch Changes

- @pnpm/fetch@2.1.9
- @pnpm/default-resolver@11.0.14
- @pnpm/tarball-fetcher@8.2.6

## 2.0.16

### Patch Changes

- Updated dependencies [263f5d813]
  - @pnpm/fetch@2.1.8
  - @pnpm/default-resolver@11.0.13
  - @pnpm/tarball-fetcher@8.2.6

## 2.0.15

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/resolver-base@7.1.0
  - @pnpm/default-resolver@11.0.12
  - @pnpm/git-fetcher@3.0.11
  - @pnpm/tarball-fetcher@8.2.6

## 2.0.14

### Patch Changes

- @pnpm/default-resolver@11.0.11

## 2.0.13

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/tarball-fetcher@8.2.5
  - @pnpm/default-resolver@11.0.10

## 2.0.12

### Patch Changes

- @pnpm/default-resolver@11.0.9

## 2.0.11

### Patch Changes

- @pnpm/default-resolver@11.0.8

## 2.0.10

### Patch Changes

- @pnpm/git-fetcher@3.0.10
- @pnpm/tarball-fetcher@8.2.4

## 2.0.9

### Patch Changes

- @pnpm/git-fetcher@3.0.10
- @pnpm/resolver-base@7.0.5
- @pnpm/fetch@2.1.7
- @pnpm/tarball-fetcher@8.2.4
- @pnpm/default-resolver@11.0.7

## 2.0.8

### Patch Changes

- Updated dependencies [212671848]
  - @pnpm/git-fetcher@3.0.9
  - @pnpm/resolver-base@7.0.4
  - @pnpm/fetch@2.1.6
  - @pnpm/tarball-fetcher@8.2.3
  - @pnpm/default-resolver@11.0.6

## 2.0.7

### Patch Changes

- 3a83db407: Update mem to v8.

## 2.0.6

### Patch Changes

- @pnpm/default-resolver@11.0.5

## 2.0.5

### Patch Changes

- @pnpm/default-resolver@11.0.4

## 2.0.4

### Patch Changes

- @pnpm/git-fetcher@3.0.8
- @pnpm/tarball-fetcher@8.2.2

## 2.0.3

### Patch Changes

- Updated dependencies [634dfd13b]
  - @pnpm/git-fetcher@3.0.7
  - @pnpm/fetch@2.1.5
  - @pnpm/tarball-fetcher@8.2.1
  - @pnpm/default-resolver@11.0.3

## 2.0.2

### Patch Changes

- Updated dependencies [7605570e6]
- Updated dependencies [3981f5558]
  - @pnpm/tarball-fetcher@8.2.0
  - @pnpm/fetch@2.1.4
  - @pnpm/default-resolver@11.0.2

## 2.0.1

### Patch Changes

- @pnpm/default-resolver@11.0.1
- @pnpm/tarball-fetcher@8.1.1

## 2.0.0

### Major Changes

- a1cdae3dc: Does not accept a `metaCache` option anymore. Caching happens internally, using `lru-cache`.

### Minor Changes

- 855f8b00a: A new function created for just creating a resolver: `createResolver(opts: ClientOptions)`.

### Patch Changes

- Updated dependencies [a1cdae3dc]
  - @pnpm/default-resolver@11.0.0

## 1.0.7

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/tarball-fetcher@8.1.0
  - @pnpm/git-fetcher@3.0.6
  - @pnpm/default-resolver@10.0.7
  - @pnpm/fetch@2.1.3

## 1.0.6

### Patch Changes

- @pnpm/default-resolver@10.0.6

## 1.0.5

### Patch Changes

- @pnpm/default-resolver@10.0.5

## 1.0.4

### Patch Changes

- Updated dependencies [a2ef8084f]
  - @pnpm/git-fetcher@3.0.6
  - @pnpm/default-resolver@10.0.4
  - @pnpm/tarball-fetcher@8.0.1

## 1.0.3

### Patch Changes

- Updated dependencies [e8a853b5b]
  - @pnpm/git-fetcher@3.0.5
  - @pnpm/fetch@2.1.2
  - @pnpm/tarball-fetcher@8.0.1
  - @pnpm/default-resolver@10.0.3

## 1.0.2

### Patch Changes

- @pnpm/default-resolver@10.0.2
- @pnpm/fetch@2.1.1
- @pnpm/tarball-fetcher@8.0.0

## 1.0.1

### Patch Changes

- @pnpm/default-resolver@10.0.1

## 1.0.0

### Major Changes

- 71aeb9a38: Initial version.

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [71aeb9a38]
- Updated dependencies [71aeb9a38]
  - @pnpm/fetch@2.1.0
  - @pnpm/fetching-types@1.0.0
  - @pnpm/default-resolver@10.0.0
  - @pnpm/tarball-fetcher@8.0.0
