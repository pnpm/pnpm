# @pnpm/plugin-commands-rebuild

## 12.0.0

### Major Changes

- dd00eeb: Renamed dir to rootDir in the Project object.

### Patch Changes

- Updated dependencies [1b03682]
- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/config@21.6.0
  - @pnpm/get-context@12.0.0
  - @pnpm/workspace.find-packages@4.0.0
  - @pnpm/types@11.0.0
  - @pnpm/cli-utils@3.1.3
  - @pnpm/store-connection-manager@8.3.1
  - @pnpm/lockfile-utils@11.0.3
  - @pnpm/store-controller-types@18.1.2
  - @pnpm/normalize-registries@6.0.3
  - @pnpm/lifecycle@17.0.7
  - @pnpm/lockfile-types@7.1.2
  - @pnpm/lockfile-walker@9.0.3
  - @pnpm/calc-dep-state@7.0.3
  - @pnpm/core-loggers@10.0.3
  - @pnpm/dependency-path@5.1.2
  - @pnpm/link-bins@10.0.5
  - @pnpm/modules-yaml@13.1.3
  - @pnpm/store.cafs@3.0.4
  - @pnpm/worker@1.0.5
  - @pnpm/sort-packages@6.0.4

## 11.3.0

### Minor Changes

- 7c6c923: Some registries allow the exact same content to be published under different package names and/or versions. This breaks the validity checks of packages in the store. To avoid errors when verifying the names and versions of such packages in the store, you may now set the `strict-store-pkg-content-check` setting to `false` [#4724](https://github.com/pnpm/pnpm/issues/4724).

### Patch Changes

- 13e55b2: If install is performed on a subset of workspace projects, always create an up-to-date lockfile first. So, a partial install can be performed only on a fully resolved (non-partial) lockfile [#8165](https://github.com/pnpm/pnpm/issues/8165).
- Updated dependencies [7c6c923]
- Updated dependencies [7d10394]
- Updated dependencies [d8eab39]
- Updated dependencies [13e55b2]
- Updated dependencies
- Updated dependencies [04b8363]
  - @pnpm/store-connection-manager@8.3.0
  - @pnpm/config@21.5.0
  - @pnpm/get-context@11.2.1
  - @pnpm/types@10.1.1
  - @pnpm/workspace.find-packages@3.0.0
  - @pnpm/cli-utils@3.1.2
  - @pnpm/normalize-registries@6.0.2
  - @pnpm/lifecycle@17.0.6
  - @pnpm/lockfile-types@7.1.1
  - @pnpm/lockfile-utils@11.0.2
  - @pnpm/lockfile-walker@9.0.2
  - @pnpm/calc-dep-state@7.0.2
  - @pnpm/core-loggers@10.0.2
  - @pnpm/dependency-path@5.1.1
  - @pnpm/link-bins@10.0.4
  - @pnpm/modules-yaml@13.1.2
  - @pnpm/store.cafs@3.0.3
  - @pnpm/store-controller-types@18.1.1
  - @pnpm/worker@1.0.4
  - @pnpm/sort-packages@6.0.3

## 11.2.0

### Minor Changes

- 47341e5: **Semi-breaking.** Dependency key names in the lockfile are shortened if they are longer than 1000 characters. We don't expect this change to affect many users. Affected users most probably can't run install successfully at the moment. This change is required to fix some edge cases in which installation fails with an out-of-memory error or "Invalid string length (RangeError: Invalid string length)" error. The max allowed length of the dependency key can be controlled with the `peers-suffix-max-length` setting [#8177](https://github.com/pnpm/pnpm/pull/8177).

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/dependency-path@5.1.0
  - @pnpm/lockfile-types@7.1.0
  - @pnpm/get-context@11.2.0
  - @pnpm/config@21.4.0
  - @pnpm/lockfile-utils@11.0.1
  - @pnpm/lockfile-walker@9.0.1
  - @pnpm/calc-dep-state@7.0.1
  - @pnpm/cli-utils@3.1.1
  - @pnpm/store-connection-manager@8.2.2
  - @pnpm/workspace.find-packages@2.1.1
  - @pnpm/lifecycle@17.0.5
  - @pnpm/link-bins@10.0.3

## 11.1.6

### Patch Changes

- Updated dependencies [80aaa9f]
- Updated dependencies [b7ca13f]
- Updated dependencies [b7ca13f]
  - @pnpm/link-bins@10.0.3
  - @pnpm/cli-utils@3.1.0
  - @pnpm/workspace.find-packages@2.1.0
  - @pnpm/config@21.3.0
  - @pnpm/lifecycle@17.0.5
  - @pnpm/store-connection-manager@8.2.1

## 11.1.5

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/store-connection-manager@8.2.0
  - @pnpm/store-controller-types@18.1.0
  - @pnpm/lifecycle@17.0.4
  - @pnpm/store.cafs@3.0.2
  - @pnpm/config@21.2.3
  - @pnpm/worker@1.0.3
  - @pnpm/cli-utils@3.0.7
  - @pnpm/workspace.find-packages@2.0.7
  - @pnpm/link-bins@10.0.2

## 11.1.4

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-types@7.0.0
  - @pnpm/lockfile-utils@11.0.0
  - @pnpm/lockfile-walker@9.0.0
  - @pnpm/calc-dep-state@7.0.0
  - @pnpm/dependency-path@5.0.0
  - @pnpm/cli-utils@3.0.6
  - @pnpm/config@21.2.2
  - @pnpm/normalize-registries@6.0.1
  - @pnpm/lifecycle@17.0.3
  - @pnpm/core-loggers@10.0.1
  - @pnpm/get-context@11.1.3
  - @pnpm/link-bins@10.0.2
  - @pnpm/modules-yaml@13.1.1
  - @pnpm/store.cafs@3.0.1
  - @pnpm/store-controller-types@18.0.1
  - @pnpm/worker@1.0.2
  - @pnpm/workspace.find-packages@2.0.6
  - @pnpm/sort-packages@6.0.2
  - @pnpm/store-connection-manager@8.1.4

## 11.1.3

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/cli-utils@3.0.5
  - @pnpm/config@21.2.1
  - @pnpm/lifecycle@17.0.2
  - @pnpm/get-context@11.1.2
  - @pnpm/link-bins@10.0.1
  - @pnpm/store-connection-manager@8.1.3
  - @pnpm/worker@1.0.1
  - @pnpm/workspace.find-packages@2.0.5

## 11.1.2

### Patch Changes

- @pnpm/cli-utils@3.0.4
- @pnpm/workspace.find-packages@2.0.4
- @pnpm/store-connection-manager@8.1.2

## 11.1.1

### Patch Changes

- Updated dependencies [db1d6ff]
- Updated dependencies [7a0536e]
  - @pnpm/deps.graph-sequencer@2.0.1
  - @pnpm/lockfile-utils@10.1.1
  - @pnpm/sort-packages@6.0.1
  - @pnpm/get-context@11.1.1
  - @pnpm/store-connection-manager@8.1.1

## 11.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/dependency-path@4.0.0
  - @pnpm/store-connection-manager@8.1.0
  - @pnpm/modules-yaml@13.1.0
  - @pnpm/lockfile-utils@10.1.0
  - @pnpm/get-context@11.1.0
  - @pnpm/config@21.2.0
  - @pnpm/lockfile-walker@8.0.1
  - @pnpm/calc-dep-state@6.0.1
  - @pnpm/cli-utils@3.0.3
  - @pnpm/workspace.find-packages@2.0.3
  - @pnpm/lifecycle@17.0.1
  - @pnpm/link-bins@10.0.0

## 11.0.5

### Patch Changes

- @pnpm/get-context@11.0.2
- @pnpm/store-connection-manager@8.0.4

## 11.0.4

### Patch Changes

- @pnpm/get-context@11.0.1

## 11.0.3

### Patch Changes

- @pnpm/store-connection-manager@8.0.3

## 11.0.2

### Patch Changes

- Updated dependencies [bfadc0a]
- Updated dependencies [a80b539]
  - @pnpm/lifecycle@17.0.1
  - @pnpm/cli-utils@3.0.2
  - @pnpm/workspace.find-packages@2.0.2
  - @pnpm/store-connection-manager@8.0.2

## 11.0.1

### Patch Changes

- Updated dependencies [e0f47f4]
  - @pnpm/config@21.1.0
  - @pnpm/cli-utils@3.0.1
  - @pnpm/store-connection-manager@8.0.1
  - @pnpm/workspace.find-packages@2.0.1

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
- Updated dependencies [cdd8365]
- Updated dependencies [c692f80]
- Updated dependencies [89b396b]
- Updated dependencies [43cdd87]
- Updated dependencies [6cdbf11]
- Updated dependencies [2d9e3b8]
- Updated dependencies [36dcaa0]
- Updated dependencies [11d9ebd]
- Updated dependencies [086b69c]
- Updated dependencies [36dcaa0]
- Updated dependencies [19c4b4f]
- Updated dependencies [d381a60]
- Updated dependencies [3477ee5]
- Updated dependencies [cfa33f1]
- Updated dependencies [e748162]
- Updated dependencies [82aac81]
- Updated dependencies [2b89155]
- Updated dependencies [27a96a8]
- Updated dependencies [60839fc]
- Updated dependencies [730929e]
- Updated dependencies [98566d9]
- Updated dependencies [98a1266]
  - @pnpm/store-connection-manager@8.0.0
  - @pnpm/types@10.0.0
  - @pnpm/config@21.0.0
  - @pnpm/error@6.0.0
  - @pnpm/worker@1.0.0
  - @pnpm/dependency-path@3.0.0
  - @pnpm/lockfile-utils@10.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/store-controller-types@18.0.0
  - @pnpm/common-cli-options-help@2.0.0
  - @pnpm/normalize-registries@6.0.0
  - @pnpm/lockfile-walker@8.0.0
  - @pnpm/modules-yaml@13.0.0
  - @pnpm/lockfile-types@6.0.0
  - @pnpm/calc-dep-state@6.0.0
  - @pnpm/get-context@11.0.0
  - @pnpm/workspace.find-packages@2.0.0
  - @pnpm/sort-packages@6.0.0
  - @pnpm/core-loggers@10.0.0
  - @pnpm/link-bins@10.0.0
  - @pnpm/deps.graph-sequencer@2.0.0
  - @pnpm/lifecycle@17.0.0
  - @pnpm/cli-utils@3.0.0
  - @pnpm/store.cafs@3.0.0

## 10.0.17

### Patch Changes

- Updated dependencies [31054a63e]
- Updated dependencies [0c383327e]
  - @pnpm/store-controller-types@17.2.0
  - @pnpm/calc-dep-state@5.0.0
  - @pnpm/lifecycle@16.0.12
  - @pnpm/store.cafs@2.0.12
  - @pnpm/lockfile-utils@9.0.5
  - @pnpm/cli-utils@2.1.9
  - @pnpm/store-connection-manager@7.0.26
  - @pnpm/config@20.4.2
  - @pnpm/worker@0.3.14
  - @pnpm/workspace.find-packages@1.1.10
  - @pnpm/link-bins@9.0.12

## 10.0.16

### Patch Changes

- Updated dependencies [60bcc797f]
  - @pnpm/get-context@10.0.11
  - @pnpm/store-connection-manager@7.0.25
  - @pnpm/lifecycle@16.0.11

## 10.0.15

### Patch Changes

- Updated dependencies [d349bc3a2]
- Updated dependencies [d9564e354]
  - @pnpm/modules-yaml@12.1.7
  - @pnpm/config@20.4.1
  - @pnpm/get-context@10.0.10
  - @pnpm/store-connection-manager@7.0.24
  - @pnpm/cli-utils@2.1.8
  - @pnpm/workspace.find-packages@1.1.9
  - @pnpm/lifecycle@16.0.10
  - @pnpm/link-bins@9.0.12

## 10.0.14

### Patch Changes

- @pnpm/worker@0.3.13
- @pnpm/store-connection-manager@7.0.23

## 10.0.13

### Patch Changes

- Updated dependencies [c597f72ec]
  - @pnpm/config@20.4.0
  - @pnpm/cli-utils@2.1.7
  - @pnpm/store-connection-manager@7.0.22
  - @pnpm/workspace.find-packages@1.1.8

## 10.0.12

### Patch Changes

- Updated dependencies [4e71066dd]
- Updated dependencies [33313d2fd]
- Updated dependencies [4d34684f1]
  - @pnpm/common-cli-options-help@1.1.0
  - @pnpm/config@20.3.0
  - @pnpm/store.cafs@2.0.11
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/types@9.4.2
  - @pnpm/cli-utils@2.1.6
  - @pnpm/store-connection-manager@7.0.21
  - @pnpm/worker@0.3.12
  - @pnpm/lockfile-utils@9.0.4
  - @pnpm/lockfile-walker@7.0.8
  - @pnpm/calc-dep-state@4.1.5
  - @pnpm/normalize-registries@5.0.6
  - @pnpm/lifecycle@16.0.10
  - @pnpm/core-loggers@9.0.6
  - @pnpm/dependency-path@2.1.7
  - @pnpm/get-context@10.0.9
  - @pnpm/link-bins@9.0.12
  - @pnpm/modules-yaml@12.1.6
  - @pnpm/store-controller-types@17.1.4
  - @pnpm/workspace.find-packages@1.1.7
  - @pnpm/sort-packages@5.0.9

## 10.0.11

### Patch Changes

- Updated dependencies
- Updated dependencies [672c559e4]
  - @pnpm/lockfile-types@5.1.4
  - @pnpm/types@9.4.1
  - @pnpm/config@20.2.0
  - @pnpm/lockfile-utils@9.0.3
  - @pnpm/lockfile-walker@7.0.7
  - @pnpm/calc-dep-state@4.1.4
  - @pnpm/cli-utils@2.1.5
  - @pnpm/normalize-registries@5.0.5
  - @pnpm/lifecycle@16.0.9
  - @pnpm/core-loggers@9.0.5
  - @pnpm/dependency-path@2.1.6
  - @pnpm/get-context@10.0.8
  - @pnpm/link-bins@9.0.11
  - @pnpm/modules-yaml@12.1.5
  - @pnpm/store.cafs@2.0.10
  - @pnpm/store-controller-types@17.1.3
  - @pnpm/worker@0.3.11
  - @pnpm/workspace.find-packages@1.1.6
  - @pnpm/sort-packages@5.0.8
  - @pnpm/store-connection-manager@7.0.20

## 10.0.10

### Patch Changes

- Updated dependencies [d5a176af7]
  - @pnpm/lockfile-utils@9.0.2
  - @pnpm/worker@0.3.10
  - @pnpm/store-connection-manager@7.0.19

## 10.0.9

### Patch Changes

- @pnpm/cli-utils@2.1.4
- @pnpm/workspace.find-packages@1.1.5
- @pnpm/store-connection-manager@7.0.18

## 10.0.8

### Patch Changes

- @pnpm/cli-utils@2.1.3
- @pnpm/workspace.find-packages@1.1.4
- @pnpm/store-connection-manager@7.0.17

## 10.0.7

### Patch Changes

- Updated dependencies [b1fd38cca]
  - @pnpm/get-context@10.0.7
  - @pnpm/store-connection-manager@7.0.16

## 10.0.6

### Patch Changes

- Updated dependencies [1e7bd4af3]
- Updated dependencies [2143a9388]
  - @pnpm/worker@0.3.9
  - @pnpm/get-context@10.0.6
  - @pnpm/workspace.find-packages@1.1.3
  - @pnpm/store-connection-manager@7.0.15

## 10.0.5

### Patch Changes

- Updated dependencies [b4194fe52]
  - @pnpm/lockfile-utils@9.0.1

## 10.0.4

### Patch Changes

- Updated dependencies [291607c5a]
  - @pnpm/store-controller-types@17.1.2
  - @pnpm/workspace.find-packages@1.1.2
  - @pnpm/lifecycle@16.0.8
  - @pnpm/store.cafs@2.0.9
  - @pnpm/store-connection-manager@7.0.14
  - @pnpm/config@20.1.2
  - @pnpm/worker@0.3.8
  - @pnpm/cli-utils@2.1.2
  - @pnpm/link-bins@9.0.10

## 10.0.3

### Patch Changes

- @pnpm/lifecycle@16.0.7
- @pnpm/store-connection-manager@7.0.13

## 10.0.2

### Patch Changes

- Updated dependencies [4c2450208]
- Updated dependencies [7ea45afbe]
  - @pnpm/lockfile-utils@9.0.0
  - @pnpm/store-controller-types@17.1.1
  - @pnpm/store-connection-manager@7.0.12
  - @pnpm/worker@0.3.7
  - @pnpm/lifecycle@16.0.6
  - @pnpm/store.cafs@2.0.8
  - @pnpm/config@20.1.1
  - @pnpm/cli-utils@2.1.1
  - @pnpm/workspace.find-packages@1.1.1
  - @pnpm/link-bins@9.0.10

## 10.0.1

### Patch Changes

- @pnpm/lifecycle@16.0.5
- @pnpm/store-connection-manager@7.0.11

## 10.0.0

### Major Changes

- d6592964f: `rootProjectManifestDir` is a required field.

### Minor Changes

- 43ce9e4a6: Support for multiple architectures when installing dependencies [#5965](https://github.com/pnpm/pnpm/issues/5965).

  You can now specify architectures for which you'd like to install optional dependencies, even if they don't match the architecture of the system running the install. Use the `supportedArchitectures` field in `package.json` to define your preferences.

  For example, the following configuration tells pnpm to install optional dependencies for Windows x64:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32"],
        "cpu": ["x64"]
      }
    }
  }
  ```

  Whereas this configuration will have pnpm install optional dependencies for Windows, macOS, and the architecture of the system currently running the install. It includes artifacts for both x64 and arm64 CPUs:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32", "darwin", "current"],
        "cpu": ["x64", "arm64"]
      }
    }
  }
  ```

  Additionally, `supportedArchitectures` also supports specifying the `libc` of the system.

- 6390033cd: Directory hard linking moved to the worker.

### Patch Changes

- Updated dependencies [43ce9e4a6]
- Updated dependencies [6390033cd]
- Updated dependencies [d6592964f]
  - @pnpm/store-controller-types@17.1.0
  - @pnpm/workspace.find-packages@1.1.0
  - @pnpm/types@9.4.0
  - @pnpm/cli-utils@2.1.0
  - @pnpm/config@20.1.0
  - @pnpm/worker@0.3.6
  - @pnpm/lifecycle@16.0.4
  - @pnpm/store.cafs@2.0.7
  - @pnpm/normalize-registries@5.0.4
  - @pnpm/lockfile-types@5.1.3
  - @pnpm/lockfile-utils@8.0.7
  - @pnpm/lockfile-walker@7.0.6
  - @pnpm/core-loggers@9.0.4
  - @pnpm/dependency-path@2.1.5
  - @pnpm/get-context@10.0.5
  - @pnpm/link-bins@9.0.10
  - @pnpm/modules-yaml@12.1.4
  - @pnpm/sort-packages@5.0.7
  - @pnpm/store-connection-manager@7.0.10
  - @pnpm/calc-dep-state@4.1.3

## 9.2.6

### Patch Changes

- Updated dependencies [5c8c9196c]
  - @pnpm/link-bins@9.0.9
  - @pnpm/lifecycle@16.0.3
  - @pnpm/store-connection-manager@7.0.9

## 9.2.5

### Patch Changes

- 4246f41be: Add package @pnpm/deps.graph-sequencer for better topological sort [#7168](https://github.com/pnpm/pnpm/pull/7168).
- Updated dependencies [4246f41be]
- Updated dependencies [84f81c9ae]
  - @pnpm/deps.graph-sequencer@1.0.0
  - @pnpm/sort-packages@5.0.6
  - @pnpm/lifecycle@16.0.2
  - @pnpm/store-connection-manager@7.0.8

## 9.2.4

### Patch Changes

- Updated dependencies [01bc58e2c]
- Updated dependencies [ac5abd3ff]
- Updated dependencies [b60bb6cbe]
  - @pnpm/store.cafs@2.0.6
  - @pnpm/config@20.0.0
  - @pnpm/store-connection-manager@7.0.7
  - @pnpm/cli-utils@2.0.24
  - @pnpm/workspace.find-packages@1.0.14
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/lifecycle@16.0.1
  - @pnpm/link-bins@9.0.8

## 9.2.3

### Patch Changes

- 4aa41ef3a: Only link bins after rebuild, when at least one dependency was actually built.
  - @pnpm/store-connection-manager@7.0.6

## 9.2.2

### Patch Changes

- @pnpm/store-connection-manager@7.0.5

## 9.2.1

### Patch Changes

- Updated dependencies [b1dd0ee58]
  - @pnpm/config@19.2.1
  - @pnpm/cli-utils@2.0.23
  - @pnpm/store-connection-manager@7.0.4
  - @pnpm/workspace.find-packages@1.0.13

## 9.2.0

### Minor Changes

- d774a3196: The list of packages that are allowed to run installation scripts now may be provided in a separate configuration file. The path to the file should be specified via the `pnpm.onlyBuiltDependenciesFile` field in `package.json`. For instance:

  ```json
  {
    "dependencies": {
      "@my-org/policy": "1.0.0"
    }
    "pnpm": {
      "onlyBuiltDependenciesFile": "node_modules/@my-org/policy/allow-build.json"
    }
  }
  ```

  In the example above, the list is loaded from a dependency. The JSON file with the list should contain an array of package names. For instance:

  ```json
  ["esbuild", "@reflink/reflink"]
  ```

  With the above list, only `esbuild` and `@reflink/reflink` will be allowed to run scripts during installation.

  Related issue: [#7137](https://github.com/pnpm/pnpm/issues/7137).

### Patch Changes

- 12f45a83d: Use `neverBuiltDependencies` and `onlyBuiltDependencies` from the root `package.json` of the workspace, when `shared-workspace-lockfile` is set to `false` [#7141](https://github.com/pnpm/pnpm/pull/7141).
- Updated dependencies [d774a3196]
- Updated dependencies [d774a3196]
- Updated dependencies [832e28826]
  - @pnpm/config@19.2.0
  - @pnpm/types@9.3.0
  - @pnpm/cli-utils@2.0.22
  - @pnpm/store-connection-manager@7.0.3
  - @pnpm/normalize-registries@5.0.3
  - @pnpm/lifecycle@16.0.1
  - @pnpm/lockfile-types@5.1.2
  - @pnpm/lockfile-utils@8.0.6
  - @pnpm/lockfile-walker@7.0.5
  - @pnpm/core-loggers@9.0.3
  - @pnpm/dependency-path@2.1.4
  - @pnpm/get-context@10.0.4
  - @pnpm/link-bins@9.0.8
  - @pnpm/modules-yaml@12.1.3
  - @pnpm/store.cafs@2.0.5
  - @pnpm/store-controller-types@17.0.1
  - @pnpm/workspace.find-packages@1.0.12
  - @pnpm/sort-packages@5.0.5
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/calc-dep-state@4.1.2

## 9.1.16

### Patch Changes

- Updated dependencies [ee328fd25]
- Updated dependencies [f394cfccd]
  - @pnpm/config@19.1.0
  - @pnpm/lockfile-utils@8.0.5
  - @pnpm/cli-utils@2.0.21
  - @pnpm/store-connection-manager@7.0.2
  - @pnpm/workspace.find-packages@1.0.11

## 9.1.15

### Patch Changes

- @pnpm/cli-utils@2.0.20
- @pnpm/workspace.find-packages@1.0.10
- @pnpm/store-connection-manager@7.0.1

## 9.1.14

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/store-connection-manager@7.0.0
  - @pnpm/store-controller-types@17.0.0
  - @pnpm/lifecycle@16.0.0
  - @pnpm/store.cafs@2.0.4
  - @pnpm/config@19.0.3
  - @pnpm/cli-utils@2.0.19
  - @pnpm/link-bins@9.0.7
  - @pnpm/workspace.find-packages@1.0.9
  - @pnpm/fs.hard-link-dir@2.0.1

## 9.1.13

### Patch Changes

- @pnpm/store-connection-manager@6.2.1

## 9.1.12

### Patch Changes

- Updated dependencies [03cdccc6e]
  - @pnpm/store-connection-manager@6.2.0
  - @pnpm/store-controller-types@16.1.0
  - @pnpm/lifecycle@15.0.9
  - @pnpm/store.cafs@2.0.3
  - @pnpm/config@19.0.2
  - @pnpm/cli-utils@2.0.18
  - @pnpm/workspace.find-packages@1.0.8
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/link-bins@9.0.6

## 9.1.11

### Patch Changes

- Updated dependencies [b3947185c]
  - @pnpm/store.cafs@2.0.2
  - @pnpm/store-connection-manager@6.1.3
  - @pnpm/config@19.0.1
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/lifecycle@15.0.8
  - @pnpm/link-bins@9.0.6

## 9.1.10

### Patch Changes

- Updated dependencies [b548f2f43]
  - @pnpm/store.cafs@2.0.1
  - @pnpm/store-connection-manager@6.1.2
  - @pnpm/lifecycle@15.0.8
  - @pnpm/store-controller-types@16.0.1
  - @pnpm/config@19.0.1
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/cli-utils@2.0.17
  - @pnpm/link-bins@9.0.6
  - @pnpm/workspace.find-packages@1.0.7

## 9.1.9

### Patch Changes

- e9aa6f682: Apply fixes from @typescript-eslint v6 for nullish coalescing and optional chains. No behavior changes are expected with this change.
- Updated dependencies [0fd9e6a6c]
- Updated dependencies [cb8bcc8df]
- Updated dependencies [494f87544]
- Updated dependencies [083bbf590]
- Updated dependencies [e9aa6f682]
  - @pnpm/store.cafs@2.0.0
  - @pnpm/config@19.0.0
  - @pnpm/store-controller-types@16.0.0
  - @pnpm/lockfile-utils@8.0.4
  - @pnpm/lifecycle@15.0.7
  - @pnpm/cli-utils@2.0.16
  - @pnpm/store-connection-manager@6.1.1
  - @pnpm/workspace.find-packages@1.0.6
  - @pnpm/link-bins@9.0.6
  - @pnpm/fs.hard-link-dir@2.0.1

## 9.1.8

### Patch Changes

- Updated dependencies [12b0f0976]
- Updated dependencies [92f42224c]
  - @pnpm/workspace.find-packages@1.0.5
  - @pnpm/store-connection-manager@6.1.0
  - @pnpm/cli-utils@2.0.15

## 9.1.7

### Patch Changes

- Updated dependencies [692197df3]
  - @pnpm/lifecycle@15.0.6
  - @pnpm/store-connection-manager@6.0.24

## 9.1.6

### Patch Changes

- @pnpm/cli-utils@2.0.14
- @pnpm/workspace.find-packages@1.0.4
- @pnpm/store-connection-manager@6.0.23

## 9.1.5

### Patch Changes

- Updated dependencies [73f2b6826]
  - @pnpm/store.cafs@1.0.2
  - @pnpm/store-connection-manager@6.0.22
  - @pnpm/config@18.4.4
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/lifecycle@15.0.5
  - @pnpm/link-bins@9.0.5

## 9.1.4

### Patch Changes

- Updated dependencies [fe1c5f48d]
  - @pnpm/store.cafs@1.0.1
  - @pnpm/store-connection-manager@6.0.21
  - @pnpm/config@18.4.4
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/lifecycle@15.0.5
  - @pnpm/link-bins@9.0.5

## 9.1.3

### Patch Changes

- Updated dependencies [4bbf482d1]
  - @pnpm/store.cafs@1.0.0
  - @pnpm/store-connection-manager@6.0.20
  - @pnpm/config@18.4.4
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/lifecycle@15.0.5
  - @pnpm/link-bins@9.0.5

## 9.1.2

### Patch Changes

- @pnpm/store-connection-manager@6.0.19

## 9.1.1

### Patch Changes

- Updated dependencies [aa2ae8fe2]
- Updated dependencies [e26d15c6d]
- Updated dependencies [250f7e9fe]
- Updated dependencies [e958707b2]
  - @pnpm/types@9.2.0
  - @pnpm/link-bins@9.0.5
  - @pnpm/cafs@7.0.5
  - @pnpm/cli-utils@2.0.13
  - @pnpm/config@18.4.4
  - @pnpm/normalize-registries@5.0.2
  - @pnpm/lifecycle@15.0.5
  - @pnpm/lockfile-types@5.1.1
  - @pnpm/lockfile-utils@8.0.3
  - @pnpm/lockfile-walker@7.0.4
  - @pnpm/core-loggers@9.0.2
  - @pnpm/dependency-path@2.1.3
  - @pnpm/get-context@10.0.3
  - @pnpm/modules-yaml@12.1.2
  - @pnpm/store-controller-types@15.0.2
  - @pnpm/workspace.find-packages@1.0.3
  - @pnpm/sort-packages@5.0.4
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/store-connection-manager@6.0.18
  - @pnpm/calc-dep-state@4.1.1

## 9.1.0

### Minor Changes

- 002f6febe: A new option added skipIfHasSideEffectsCache for skipping the build for dependencies that already have side effects cache.

### Patch Changes

- 16bbac8d5: `pnpm rebuild` should upload the built artifacts to the content-addressable store.
- Updated dependencies [16bbac8d5]
  - @pnpm/calc-dep-state@4.1.0

## 9.0.7

### Patch Changes

- @pnpm/cli-utils@2.0.12
- @pnpm/config@18.4.3
- @pnpm/link-bins@9.0.4
- @pnpm/workspace.find-packages@1.0.2
- @pnpm/store-connection-manager@6.0.17
- @pnpm/lifecycle@15.0.4

## 9.0.6

### Patch Changes

- dddb8ad71: Local workspace bin files that should be compiled first are linked to dependent projects after compilation [#1801](https://github.com/pnpm/pnpm/issues/1801).
- Updated dependencies [dddb8ad71]
  - @pnpm/lifecycle@15.0.3
  - @pnpm/store-connection-manager@6.0.16
  - @pnpm/config@18.4.2
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/link-bins@9.0.3

## 9.0.5

### Patch Changes

- @pnpm/store-connection-manager@6.0.15

## 9.0.4

### Patch Changes

- @pnpm/store-connection-manager@6.0.14

## 9.0.3

### Patch Changes

- Updated dependencies [e2d631217]
  - @pnpm/config@18.4.2
  - @pnpm/cli-utils@2.0.11
  - @pnpm/store-connection-manager@6.0.13
  - @pnpm/workspace.find-packages@1.0.1
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/lifecycle@15.0.2
  - @pnpm/link-bins@9.0.3

## 9.0.2

### Patch Changes

- Updated dependencies [bd235794d]
- Updated dependencies [d9da627cd]
- Updated dependencies [302ebffc5]
- Updated dependencies [bd235794d]
  - @pnpm/workspace.find-packages@1.0.0
  - @pnpm/lockfile-utils@8.0.2
  - @pnpm/constants@7.1.1
  - @pnpm/config@18.4.1
  - @pnpm/error@5.0.2
  - @pnpm/get-context@10.0.2
  - @pnpm/cli-utils@2.0.10
  - @pnpm/store-connection-manager@6.0.12
  - @pnpm/lifecycle@15.0.2
  - @pnpm/link-bins@9.0.3
  - @pnpm/fs.hard-link-dir@2.0.1

## 9.0.1

### Patch Changes

- Updated dependencies [4b97f1f07]
  - @pnpm/get-context@10.0.1
  - @pnpm/link-bins@9.0.2
  - @pnpm/store-connection-manager@6.0.11
  - @pnpm/config@18.4.0
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/lifecycle@15.0.1

## 9.0.0

### Major Changes

- 9c4ae87bd: New required options added: autoInstallPeers and excludeLinksFromLockfile.

### Patch Changes

- Updated dependencies [a9e0b7cbf]
- Updated dependencies [a53ef4d19]
- Updated dependencies [9c4ae87bd]
- Updated dependencies [301b8e2da]
- Updated dependencies [9c4ae87bd]
- Updated dependencies [6ce3424a9]
  - @pnpm/types@9.1.0
  - @pnpm/get-context@10.0.0
  - @pnpm/constants@7.1.0
  - @pnpm/config@18.4.0
  - @pnpm/lifecycle@15.0.1
  - @pnpm/lockfile-utils@8.0.1
  - @pnpm/lockfile-walker@7.0.3
  - @pnpm/cli-utils@2.0.9
  - @pnpm/normalize-registries@5.0.1
  - @pnpm/core-loggers@9.0.1
  - @pnpm/dependency-path@2.1.2
  - @pnpm/link-bins@9.0.1
  - @pnpm/modules-yaml@12.1.1
  - @pnpm/store-controller-types@15.0.1
  - @pnpm/find-workspace-packages@6.0.9
  - @pnpm/sort-packages@5.0.3
  - @pnpm/error@5.0.1
  - @pnpm/store-connection-manager@6.0.10
  - @pnpm/fs.hard-link-dir@2.0.1

## 8.0.11

### Patch Changes

- ea28b6f6c: `pnpm rebuild` should not fail when `node-linker` is set to `hoisted` and there are skipped optional dependencies [#6553](https://github.com/pnpm/pnpm/pull/6553).

## 8.0.10

### Patch Changes

- Updated dependencies [d58cdb962]
- Updated dependencies [ee429b300]
- Updated dependencies [1de07a4af]
  - @pnpm/lockfile-utils@8.0.0
  - @pnpm/cli-utils@2.0.8
  - @pnpm/config@18.3.2
  - @pnpm/find-workspace-packages@6.0.8
  - @pnpm/store-connection-manager@6.0.9

## 8.0.9

### Patch Changes

- 3fa14d7e4: Show cyclic workspace dependency details [#5059](https://github.com/pnpm/pnpm/issues/5059).
- Updated dependencies [1ffedcb8d]
- Updated dependencies [3fa14d7e4]
  - @pnpm/get-context@9.1.0
  - @pnpm/sort-packages@5.0.2

## 8.0.8

### Patch Changes

- Updated dependencies [497b0a79c]
- Updated dependencies [2809e89ab]
- Updated dependencies [e6b83c84e]
  - @pnpm/get-context@9.0.4
  - @pnpm/config@18.3.1
  - @pnpm/modules-yaml@12.1.0
  - @pnpm/cli-utils@2.0.7
  - @pnpm/store-connection-manager@6.0.8
  - @pnpm/find-workspace-packages@6.0.7
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/lifecycle@15.0.0
  - @pnpm/link-bins@9.0.0

## 8.0.7

### Patch Changes

- @pnpm/store-connection-manager@6.0.7

## 8.0.6

### Patch Changes

- d43ccc44d: Update `@pnpm/graph-sequencer`.
- c0760128d: bump semver to 7.4.0
- Updated dependencies [64d0f47ff]
- Updated dependencies [32f8e08c6]
- Updated dependencies [d43ccc44d]
- Updated dependencies [c0760128d]
  - @pnpm/fs.hard-link-dir@2.0.1
  - @pnpm/config@18.3.0
  - @pnpm/sort-packages@5.0.1
  - @pnpm/dependency-path@2.1.1
  - @pnpm/cli-utils@2.0.6
  - @pnpm/store-connection-manager@6.0.6
  - @pnpm/lockfile-utils@7.0.1
  - @pnpm/lockfile-walker@7.0.2
  - @pnpm/get-context@9.0.3
  - @pnpm/find-workspace-packages@6.0.6

## 8.0.5

### Patch Changes

- Updated dependencies [72ba638e3]
- Updated dependencies [fc8780ca9]
- Updated dependencies [080fee0b8]
  - @pnpm/lockfile-utils@7.0.0
  - @pnpm/config@18.2.0
  - @pnpm/get-context@9.0.2
  - @pnpm/cli-utils@2.0.5
  - @pnpm/store-connection-manager@6.0.5
  - @pnpm/find-workspace-packages@6.0.5

## 8.0.4

### Patch Changes

- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
  - @pnpm/dependency-path@2.1.0
  - @pnpm/lockfile-utils@6.0.1
  - @pnpm/lockfile-walker@7.0.1
  - @pnpm/get-context@9.0.1
  - @pnpm/cli-utils@2.0.4
  - @pnpm/config@18.1.1
  - @pnpm/find-workspace-packages@6.0.4
  - @pnpm/store-connection-manager@6.0.4

## 8.0.3

### Patch Changes

- Updated dependencies [e2cb4b63d]
- Updated dependencies [cd6ce11f0]
  - @pnpm/config@18.1.0
  - @pnpm/cli-utils@2.0.3
  - @pnpm/store-connection-manager@6.0.3
  - @pnpm/find-workspace-packages@6.0.3

## 8.0.2

### Patch Changes

- @pnpm/config@18.0.2
- @pnpm/cli-utils@2.0.2
- @pnpm/store-connection-manager@6.0.2
- @pnpm/find-workspace-packages@6.0.2

## 8.0.1

### Patch Changes

- @pnpm/config@18.0.1
- @pnpm/cli-utils@2.0.1
- @pnpm/store-connection-manager@6.0.1
- @pnpm/find-workspace-packages@6.0.1

## 8.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [47e45d717]
- Updated dependencies [c92936158]
- Updated dependencies [47e45d717]
- Updated dependencies [2a2032810]
- Updated dependencies [7a0ce1df0]
- Updated dependencies [158d8cf22]
- Updated dependencies [ca8f51e60]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [8e35c21d1]
- Updated dependencies [0e26acb0f]
- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [113f0ae26]
  - @pnpm/config@18.0.0
  - @pnpm/lockfile-utils@6.0.0
  - @pnpm/get-context@9.0.0
  - @pnpm/store-connection-manager@6.0.0
  - @pnpm/dependency-path@2.0.0
  - @pnpm/find-workspace-packages@6.0.0
  - @pnpm/store-controller-types@15.0.0
  - @pnpm/common-cli-options-help@1.0.0
  - @pnpm/normalize-registries@5.0.0
  - @pnpm/lockfile-walker@7.0.0
  - @pnpm/modules-yaml@12.0.0
  - @pnpm/sort-packages@5.0.0
  - @pnpm/core-loggers@9.0.0
  - @pnpm/link-bins@9.0.0
  - @pnpm/constants@7.0.0
  - @pnpm/fs.hard-link-dir@2.0.0
  - @pnpm/lifecycle@15.0.0
  - @pnpm/types@9.0.0
  - @pnpm/cli-utils@2.0.0

## 7.1.7

### Patch Changes

- Updated dependencies [685b3a7ea]
  - @pnpm/link-bins@8.0.11
  - @pnpm/config@17.0.2
  - @pnpm/cli-utils@1.1.7
  - @pnpm/store-connection-manager@5.2.20
  - @pnpm/find-workspace-packages@5.0.42

## 7.1.6

### Patch Changes

- Updated dependencies [f9c30c6d7]
- Updated dependencies [b38d711f3]
  - @pnpm/link-bins@8.0.10
  - @pnpm/config@17.0.1
  - @pnpm/cli-utils@1.1.6
  - @pnpm/store-connection-manager@5.2.19
  - @pnpm/find-workspace-packages@5.0.41

## 7.1.5

### Patch Changes

- Updated dependencies [e505b58e3]
  - @pnpm/config@17.0.0
  - @pnpm/get-context@8.2.4
  - @pnpm/cli-utils@1.1.5
  - @pnpm/store-connection-manager@5.2.18
  - @pnpm/link-bins@8.0.9
  - @pnpm/find-workspace-packages@5.0.40
  - @pnpm/lifecycle@14.1.7
  - @pnpm/fs.hard-link-dir@1.0.3

## 7.1.4

### Patch Changes

- @pnpm/config@16.7.2
- @pnpm/cli-utils@1.1.4
- @pnpm/store-connection-manager@5.2.17
- @pnpm/find-workspace-packages@5.0.39

## 7.1.3

### Patch Changes

- @pnpm/config@16.7.1
- @pnpm/cli-utils@1.1.3
- @pnpm/store-connection-manager@5.2.16
- @pnpm/find-workspace-packages@5.0.38

## 7.1.2

### Patch Changes

- Updated dependencies [7d64d757b]
- Updated dependencies [5c31fa8be]
  - @pnpm/cli-utils@1.1.2
  - @pnpm/config@16.7.0
  - @pnpm/find-workspace-packages@5.0.37
  - @pnpm/store-connection-manager@5.2.15

## 7.1.1

### Patch Changes

- @pnpm/get-context@8.2.3
- @pnpm/config@16.6.4
- @pnpm/cli-utils@1.1.1
- @pnpm/store-connection-manager@5.2.14
- @pnpm/find-workspace-packages@5.0.36

## 7.1.0

### Minor Changes

- 0377d9367: Add --report-summary for pnpm exec and pnpm run [#6008](https://github.com/pnpm/pnpm/issues/6008)

### Patch Changes

- Updated dependencies [0377d9367]
  - @pnpm/cli-utils@1.1.0
  - @pnpm/find-workspace-packages@5.0.35
  - @pnpm/config@16.6.3
  - @pnpm/store-connection-manager@5.2.13

## 7.0.34

### Patch Changes

- @pnpm/store-connection-manager@5.2.12
- @pnpm/config@16.6.2
- @pnpm/cli-utils@1.0.34
- @pnpm/find-workspace-packages@5.0.34

## 7.0.33

### Patch Changes

- @pnpm/lockfile-utils@5.0.7
- @pnpm/store-controller-types@14.3.1
- @pnpm/config@16.6.1
- @pnpm/fs.hard-link-dir@1.0.3
- @pnpm/lifecycle@14.1.6
- @pnpm/store-connection-manager@5.2.11
- @pnpm/link-bins@8.0.8
- @pnpm/cli-utils@1.0.33
- @pnpm/find-workspace-packages@5.0.33

## 7.0.32

### Patch Changes

- Updated dependencies [d89d7a078]
- Updated dependencies [59ee53678]
  - @pnpm/dependency-path@1.1.3
  - @pnpm/config@16.6.0
  - @pnpm/lockfile-utils@5.0.6
  - @pnpm/lockfile-walker@6.0.8
  - @pnpm/cli-utils@1.0.32
  - @pnpm/store-connection-manager@5.2.10
  - @pnpm/get-context@8.2.2
  - @pnpm/find-workspace-packages@5.0.32

## 7.0.31

### Patch Changes

- Updated dependencies [6add01403]
- Updated dependencies [5c4eb0fc3]
- Updated dependencies [9247f6781]
  - @pnpm/fs.hard-link-dir@1.0.3
  - @pnpm/dependency-path@1.1.2
  - @pnpm/lockfile-utils@5.0.5
  - @pnpm/lockfile-walker@6.0.7
  - @pnpm/get-context@8.2.1
  - @pnpm/config@16.5.5
  - @pnpm/store-connection-manager@5.2.9
  - @pnpm/cli-utils@1.0.31
  - @pnpm/find-workspace-packages@5.0.31

## 7.0.30

### Patch Changes

- Updated dependencies [78d4cf1f7]
  - @pnpm/fs.hard-link-dir@1.0.2
  - @pnpm/store-connection-manager@5.2.8
  - @pnpm/config@16.5.4
  - @pnpm/cli-utils@1.0.30
  - @pnpm/find-workspace-packages@5.0.30

## 7.0.29

### Patch Changes

- @pnpm/config@16.5.3
- @pnpm/cli-utils@1.0.29
- @pnpm/store-connection-manager@5.2.7
- @pnpm/find-workspace-packages@5.0.29

## 7.0.28

### Patch Changes

- @pnpm/config@16.5.2
- @pnpm/cli-utils@1.0.28
- @pnpm/store-connection-manager@5.2.6
- @pnpm/find-workspace-packages@5.0.28

## 7.0.27

### Patch Changes

- Updated dependencies [90d26c449]
- Updated dependencies [00d86db16]
  - @pnpm/link-bins@8.0.8
  - @pnpm/fs.hard-link-dir@1.0.1
  - @pnpm/store-connection-manager@5.2.5
  - @pnpm/config@16.5.1
  - @pnpm/cli-utils@1.0.27
  - @pnpm/lifecycle@14.1.5
  - @pnpm/find-workspace-packages@5.0.27

## 7.0.26

### Patch Changes

- Updated dependencies [28b47a156]
- Updated dependencies [4008a5236]
  - @pnpm/get-context@8.2.0
  - @pnpm/config@16.5.0
  - @pnpm/link-bins@8.0.7
  - @pnpm/cli-utils@1.0.26
  - @pnpm/store-connection-manager@5.2.4
  - @pnpm/find-workspace-packages@5.0.26

## 7.0.25

### Patch Changes

- @pnpm/store-connection-manager@5.2.3
- @pnpm/config@16.4.3
- @pnpm/cli-utils@1.0.25
- @pnpm/fs.hard-link-dir@1.0.0
- @pnpm/find-workspace-packages@5.0.25
- @pnpm/lifecycle@14.1.5
- @pnpm/link-bins@8.0.6

## 7.0.24

### Patch Changes

- @pnpm/get-context@8.1.2
- @pnpm/config@16.4.2
- @pnpm/cli-utils@1.0.24
- @pnpm/store-connection-manager@5.2.2
- @pnpm/find-workspace-packages@5.0.24

## 7.0.23

### Patch Changes

- Updated dependencies [0f6e95872]
  - @pnpm/dependency-path@1.1.1
  - @pnpm/lockfile-utils@5.0.4
  - @pnpm/lockfile-walker@6.0.6
  - @pnpm/get-context@8.1.1
  - @pnpm/config@16.4.1
  - @pnpm/store-connection-manager@5.2.1
  - @pnpm/cli-utils@1.0.23
  - @pnpm/find-workspace-packages@5.0.23

## 7.0.22

### Patch Changes

- Updated dependencies [891a8d763]
- Updated dependencies [c7b05cd9a]
- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
  - @pnpm/store-controller-types@14.3.0
  - @pnpm/store-connection-manager@5.2.0
  - @pnpm/constants@6.2.0
  - @pnpm/get-context@8.1.0
  - @pnpm/config@16.4.0
  - @pnpm/dependency-path@1.1.0
  - @pnpm/lifecycle@14.1.5
  - @pnpm/find-workspace-packages@5.0.22
  - @pnpm/cli-utils@1.0.22
  - @pnpm/lockfile-utils@5.0.3
  - @pnpm/lockfile-walker@6.0.5
  - @pnpm/link-bins@8.0.6
  - @pnpm/fs.hard-link-dir@1.0.0

## 7.0.21

### Patch Changes

- Updated dependencies [1fad508b0]
  - @pnpm/config@16.3.0
  - @pnpm/cli-utils@1.0.21
  - @pnpm/store-connection-manager@5.1.14
  - @pnpm/find-workspace-packages@5.0.21

## 7.0.20

### Patch Changes

- Updated dependencies [ec97a3105]
- Updated dependencies [08ceaf3fc]
  - @pnpm/store-connection-manager@5.1.13
  - @pnpm/get-context@8.0.6
  - @pnpm/cli-utils@1.0.20
  - @pnpm/find-workspace-packages@5.0.20
  - @pnpm/config@16.2.2

## 7.0.19

### Patch Changes

- Updated dependencies [d71dbf230]
  - @pnpm/config@16.2.1
  - @pnpm/cli-utils@1.0.19
  - @pnpm/store-connection-manager@5.1.12
  - @pnpm/find-workspace-packages@5.0.19

## 7.0.18

### Patch Changes

- 5c75ff3ec: `pnpm rebuild` should not fail if node_modules was created by pnpm version 7.18 or older [#5815](https://github.com/pnpm/pnpm/issues/5815).
- 5c75ff3ec: `pnpm rebuild` should print a better error message when a hoisted dependency is not found [#5815](https://github.com/pnpm/pnpm/issues/5815).
- Updated dependencies [841f52e70]
  - @pnpm/config@16.2.0
  - @pnpm/store-connection-manager@5.1.11
  - @pnpm/cli-utils@1.0.18
  - @pnpm/find-workspace-packages@5.0.18

## 7.0.17

### Patch Changes

- c9d3970e3: `pnpm rebuild` should work in projects that use the hoister node linker [#5560](https://github.com/pnpm/pnpm/issues/5560).
- Updated dependencies [b77651d14]
- Updated dependencies [bc18d33fe]
- Updated dependencies [2458741fa]
- Updated dependencies [c9d3970e3]
- Updated dependencies [6b00a8325]
- Updated dependencies [2458741fa]
  - @pnpm/types@8.10.0
  - @pnpm/link-bins@8.0.5
  - @pnpm/store-controller-types@14.2.0
  - @pnpm/fs.hard-link-dir@1.0.0
  - @pnpm/lifecycle@14.1.4
  - @pnpm/modules-yaml@11.1.0
  - @pnpm/cli-utils@1.0.17
  - @pnpm/config@16.1.11
  - @pnpm/normalize-registries@4.0.3
  - @pnpm/lockfile-utils@5.0.2
  - @pnpm/lockfile-walker@6.0.4
  - @pnpm/core-loggers@8.0.3
  - @pnpm/dependency-path@1.0.1
  - @pnpm/get-context@8.0.5
  - @pnpm/find-workspace-packages@5.0.17
  - @pnpm/sort-packages@4.0.3
  - @pnpm/store-connection-manager@5.1.10

## 7.0.16

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/dependency-path@1.0.0
  - @pnpm/lockfile-utils@5.0.1
  - @pnpm/lockfile-walker@6.0.3
  - @pnpm/find-workspace-packages@5.0.16
  - @pnpm/config@16.1.10
  - @pnpm/get-context@8.0.4
  - @pnpm/cli-utils@1.0.16
  - @pnpm/store-connection-manager@5.1.9

## 7.0.15

### Patch Changes

- Updated dependencies [f5c377a8d]
  - @pnpm/lifecycle@14.1.3
  - @pnpm/config@16.1.9
  - @pnpm/cli-utils@1.0.15
  - @pnpm/store-connection-manager@5.1.8
  - @pnpm/find-workspace-packages@5.0.15

## 7.0.14

### Patch Changes

- @pnpm/cli-utils@1.0.14
- @pnpm/find-workspace-packages@5.0.14
- @pnpm/config@16.1.8
- @pnpm/store-connection-manager@5.1.7

## 7.0.13

### Patch Changes

- Updated dependencies [a9d59d8bc]
- Updated dependencies [93558ce68]
  - @pnpm/config@16.1.7
  - @pnpm/link-bins@8.0.4
  - @pnpm/lifecycle@14.1.2
  - @pnpm/cli-utils@1.0.13
  - @pnpm/store-connection-manager@5.1.6
  - @pnpm/get-context@8.0.3
  - @pnpm/find-workspace-packages@5.0.13

## 7.0.12

### Patch Changes

- @pnpm/config@16.1.6
- @pnpm/cli-utils@1.0.12
- @pnpm/store-connection-manager@5.1.5
- @pnpm/find-workspace-packages@5.0.12

## 7.0.11

### Patch Changes

- @pnpm/config@16.1.5
- @pnpm/cli-utils@1.0.11
- @pnpm/store-connection-manager@5.1.4
- @pnpm/find-workspace-packages@5.0.11

## 7.0.10

### Patch Changes

- @pnpm/cli-utils@1.0.10
- @pnpm/config@16.1.4
- @pnpm/link-bins@8.0.3
- @pnpm/find-workspace-packages@5.0.10
- @pnpm/store-connection-manager@5.1.3
- @pnpm/lifecycle@14.1.1

## 7.0.9

### Patch Changes

- @pnpm/config@16.1.3
- @pnpm/cli-utils@1.0.9
- @pnpm/store-connection-manager@5.1.2
- @pnpm/find-workspace-packages@5.0.9

## 7.0.8

### Patch Changes

- Updated dependencies [1d04e663b]
  - @pnpm/lifecycle@14.1.0
  - @pnpm/config@16.1.2
  - @pnpm/cli-utils@1.0.8
  - @pnpm/store-connection-manager@5.1.1
  - @pnpm/find-workspace-packages@5.0.8

## 7.0.7

### Patch Changes

- Updated dependencies [eacff33e4]
- Updated dependencies [ecc8794bb]
- Updated dependencies [ecc8794bb]
  - @pnpm/store-connection-manager@5.1.0
  - @pnpm/lockfile-utils@5.0.0
  - @pnpm/lifecycle@14.0.3
  - @pnpm/config@16.1.1
  - @pnpm/cli-utils@1.0.7
  - @pnpm/find-workspace-packages@5.0.7

## 7.0.6

### Patch Changes

- 3dab7f83c: Replace environment variable placeholders with their values, when reading `.npmrc` files in subdirectories inside a workspace [#2570](https://github.com/pnpm/pnpm/issues/2570).
- Updated dependencies [3dab7f83c]
  - @pnpm/config@16.1.0
  - @pnpm/cli-utils@1.0.6
  - @pnpm/store-connection-manager@5.0.6
  - @pnpm/find-workspace-packages@5.0.6

## 7.0.5

### Patch Changes

- Updated dependencies [a4c58d424]
- Updated dependencies [2e9790722]
- Updated dependencies [702e847c1]
  - @pnpm/lifecycle@14.0.2
  - @pnpm/find-workspace-packages@5.0.5
  - @pnpm/types@8.9.0
  - @pnpm/cli-utils@1.0.5
  - @pnpm/config@16.0.5
  - @pnpm/core-loggers@8.0.2
  - dependency-path@9.2.8
  - @pnpm/get-context@8.0.2
  - @pnpm/link-bins@8.0.2
  - @pnpm/lockfile-utils@4.2.8
  - @pnpm/lockfile-walker@6.0.2
  - @pnpm/modules-yaml@11.0.2
  - @pnpm/normalize-registries@4.0.2
  - @pnpm/sort-packages@4.0.2
  - @pnpm/store-controller-types@14.1.5
  - @pnpm/store-connection-manager@5.0.5

## 7.0.4

### Patch Changes

- @pnpm/config@16.0.4
- @pnpm/cli-utils@1.0.4
- @pnpm/store-connection-manager@5.0.4
- @pnpm/find-workspace-packages@5.0.4

## 7.0.3

### Patch Changes

- Updated dependencies [aacb83f73]
- Updated dependencies [a14ad09e6]
  - @pnpm/config@16.0.3
  - @pnpm/cli-utils@1.0.3
  - @pnpm/store-connection-manager@5.0.3
  - @pnpm/find-workspace-packages@5.0.3

## 7.0.2

### Patch Changes

- Updated dependencies [bea0acdfc]
  - @pnpm/config@16.0.2
  - @pnpm/cli-utils@1.0.2
  - @pnpm/store-connection-manager@5.0.2
  - @pnpm/find-workspace-packages@5.0.2

## 7.0.1

### Patch Changes

- Updated dependencies [e7fd8a84c]
- Updated dependencies [844e82f3a]
  - @pnpm/config@16.0.1
  - @pnpm/types@8.8.0
  - @pnpm/cli-utils@1.0.1
  - @pnpm/store-connection-manager@5.0.1
  - @pnpm/core-loggers@8.0.1
  - dependency-path@9.2.7
  - @pnpm/find-workspace-packages@5.0.1
  - @pnpm/get-context@8.0.1
  - @pnpm/lifecycle@14.0.1
  - @pnpm/link-bins@8.0.1
  - @pnpm/lockfile-utils@4.2.7
  - @pnpm/lockfile-walker@6.0.1
  - @pnpm/modules-yaml@11.0.1
  - @pnpm/normalize-registries@4.0.1
  - @pnpm/sort-packages@4.0.1
  - @pnpm/store-controller-types@14.1.4

## 7.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [645384bfd]
- Updated dependencies [1d0fd82fd]
- Updated dependencies [645384bfd]
- Updated dependencies [72f7d6b3b]
- Updated dependencies [f884689e0]
- Updated dependencies [3c117996e]
  - @pnpm/config@16.0.0
  - @pnpm/find-workspace-packages@5.0.0
  - @pnpm/lifecycle@14.0.0
  - @pnpm/link-bins@8.0.0
  - @pnpm/lockfile-walker@6.0.0
  - @pnpm/get-context@8.0.0
  - @pnpm/modules-yaml@11.0.0
  - @pnpm/cli-utils@1.0.0
  - @pnpm/core-loggers@8.0.0
  - @pnpm/normalize-registries@4.0.0
  - @pnpm/sort-packages@4.0.0
  - @pnpm/store-connection-manager@5.0.0

## 6.2.12

### Patch Changes

- @pnpm/store-connection-manager@4.3.16
- @pnpm/get-context@7.0.3
- @pnpm/config@15.10.12
- @pnpm/cli-utils@0.7.43
- @pnpm/link-bins@7.2.10
- @pnpm/find-workspace-packages@4.0.43
- @pnpm/lifecycle@13.1.12

## 6.2.11

### Patch Changes

- Updated dependencies [3ae888c28]
  - @pnpm/core-loggers@7.1.0
  - @pnpm/get-context@7.0.2
  - @pnpm/lifecycle@13.1.11
  - @pnpm/cli-utils@0.7.42
  - @pnpm/link-bins@7.2.9
  - @pnpm/config@15.10.11
  - @pnpm/find-workspace-packages@4.0.42
  - @pnpm/store-connection-manager@4.3.15

## 6.2.10

### Patch Changes

- Updated dependencies [5eb41a551]
  - @pnpm/link-bins@7.2.8
  - @pnpm/cli-utils@0.7.41
  - @pnpm/config@15.10.10
  - @pnpm/get-context@7.0.1
  - @pnpm/store-connection-manager@4.3.14
  - @pnpm/find-workspace-packages@4.0.41
  - @pnpm/lifecycle@13.1.10

## 6.2.9

### Patch Changes

- Updated dependencies [d665f3ff7]
- Updated dependencies [51566e34b]
  - @pnpm/types@8.7.0
  - @pnpm/get-context@7.0.0
  - @pnpm/config@15.10.9
  - @pnpm/cli-utils@0.7.40
  - @pnpm/core-loggers@7.0.8
  - dependency-path@9.2.6
  - @pnpm/find-workspace-packages@4.0.40
  - @pnpm/lifecycle@13.1.9
  - @pnpm/link-bins@7.2.7
  - @pnpm/lockfile-utils@4.2.6
  - @pnpm/lockfile-walker@5.0.15
  - @pnpm/modules-yaml@10.0.8
  - @pnpm/normalize-registries@3.0.8
  - @pnpm/sort-packages@3.0.9
  - @pnpm/store-controller-types@14.1.3
  - @pnpm/store-connection-manager@4.3.13

## 6.2.8

### Patch Changes

- @pnpm/config@15.10.8
- @pnpm/cli-utils@0.7.39
- @pnpm/store-connection-manager@4.3.12
- @pnpm/find-workspace-packages@4.0.39

## 6.2.7

### Patch Changes

- @pnpm/config@15.10.7
- @pnpm/cli-utils@0.7.38
- @pnpm/store-connection-manager@4.3.11
- @pnpm/find-workspace-packages@4.0.38

## 6.2.6

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/cli-utils@0.7.37
  - @pnpm/config@15.10.6
  - @pnpm/core-loggers@7.0.7
  - dependency-path@9.2.5
  - @pnpm/find-workspace-packages@4.0.37
  - @pnpm/get-context@6.2.11
  - @pnpm/lifecycle@13.1.8
  - @pnpm/link-bins@7.2.6
  - @pnpm/lockfile-utils@4.2.5
  - @pnpm/lockfile-walker@5.0.14
  - @pnpm/modules-yaml@10.0.7
  - @pnpm/normalize-registries@3.0.7
  - @pnpm/sort-packages@3.0.8
  - @pnpm/store-controller-types@14.1.2
  - @pnpm/store-connection-manager@4.3.10

## 6.2.5

### Patch Changes

- @pnpm/store-connection-manager@4.3.9
- @pnpm/config@15.10.5
- @pnpm/cli-utils@0.7.36
- @pnpm/find-workspace-packages@4.0.36

## 6.2.4

### Patch Changes

- @pnpm/cli-utils@0.7.35
- @pnpm/config@15.10.4
- @pnpm/find-workspace-packages@4.0.35
- @pnpm/store-connection-manager@4.3.8

## 6.2.3

### Patch Changes

- @pnpm/get-context@6.2.10
- @pnpm/store-connection-manager@4.3.7
- @pnpm/config@15.10.3
- @pnpm/cli-utils@0.7.34
- @pnpm/find-workspace-packages@4.0.34

## 6.2.2

### Patch Changes

- @pnpm/store-connection-manager@4.3.6
- @pnpm/config@15.10.2
- @pnpm/cli-utils@0.7.33
- @pnpm/find-workspace-packages@4.0.33

## 6.2.1

### Patch Changes

- Updated dependencies [17e69e18b]
  - @pnpm/store-connection-manager@4.3.5
  - @pnpm/config@15.10.1
  - @pnpm/cli-utils@0.7.32
  - @pnpm/find-workspace-packages@4.0.32

## 6.2.0

### Minor Changes

- 2aa22e4b1: Set `NODE_PATH` when `preferSymlinkedExecutables` is enabled.

### Patch Changes

- Updated dependencies [2aa22e4b1]
  - @pnpm/config@15.10.0
  - @pnpm/cli-utils@0.7.31
  - @pnpm/store-connection-manager@4.3.4
  - @pnpm/find-workspace-packages@4.0.31

## 6.1.34

### Patch Changes

- @pnpm/config@15.9.4
- @pnpm/cli-utils@0.7.30
- @pnpm/store-connection-manager@4.3.3
- @pnpm/find-workspace-packages@4.0.30

## 6.1.33

### Patch Changes

- @pnpm/store-controller-types@14.1.1
- @pnpm/store-connection-manager@4.3.2
- @pnpm/config@15.9.3
- @pnpm/cli-utils@0.7.29
- @pnpm/link-bins@7.2.5
- @pnpm/find-workspace-packages@4.0.29

## 6.1.32

### Patch Changes

- Updated dependencies [07bc24ad1]
  - @pnpm/link-bins@7.2.5
  - @pnpm/store-connection-manager@4.3.1
  - @pnpm/lifecycle@13.1.7
  - @pnpm/config@15.9.2
  - @pnpm/cli-utils@0.7.28
  - @pnpm/find-workspace-packages@4.0.28

## 6.1.31

### Patch Changes

- Updated dependencies [32915f0e4]
- Updated dependencies [23984abd1]
  - @pnpm/store-controller-types@14.1.1
  - @pnpm/store-connection-manager@4.3.0
  - @pnpm/lifecycle@13.1.6
  - @pnpm/config@15.9.1
  - @pnpm/lockfile-utils@4.2.4
  - @pnpm/cli-utils@0.7.27
  - @pnpm/find-workspace-packages@4.0.27
  - @pnpm/link-bins@7.2.4

## 6.1.30

### Patch Changes

- @pnpm/get-context@6.2.9
- @pnpm/store-connection-manager@4.2.1
- @pnpm/config@15.9.0
- @pnpm/link-bins@7.2.4

## 6.1.29

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [43cd6aaca]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
- Updated dependencies [29a81598a]
  - @pnpm/config@15.9.0
  - @pnpm/get-context@6.2.8
  - @pnpm/link-bins@7.2.4
  - @pnpm/lockfile-utils@4.2.3
  - @pnpm/lockfile-walker@5.0.13
  - @pnpm/store-connection-manager@4.2.0
  - @pnpm/store-controller-types@14.1.0
  - @pnpm/cli-utils@0.7.26
  - @pnpm/lifecycle@13.1.5
  - @pnpm/find-workspace-packages@4.0.26

## 6.1.28

### Patch Changes

- Updated dependencies [c90798461]
- Updated dependencies [34121d753]
  - @pnpm/types@8.5.0
  - @pnpm/config@15.8.1
  - @pnpm/get-context@6.2.7
  - @pnpm/cli-utils@0.7.25
  - @pnpm/core-loggers@7.0.6
  - dependency-path@9.2.4
  - @pnpm/find-workspace-packages@4.0.25
  - @pnpm/lifecycle@13.1.4
  - @pnpm/link-bins@7.2.3
  - @pnpm/lockfile-utils@4.2.2
  - @pnpm/lockfile-walker@5.0.12
  - @pnpm/modules-yaml@10.0.6
  - @pnpm/normalize-registries@3.0.6
  - @pnpm/sort-packages@3.0.7
  - @pnpm/store-controller-types@14.0.2
  - @pnpm/store-connection-manager@4.1.26

## 6.1.27

### Patch Changes

- Updated dependencies [c83f40c10]
  - @pnpm/lockfile-utils@4.2.1

## 6.1.26

### Patch Changes

- Updated dependencies [cac34ad69]
- Updated dependencies [99019e071]
  - @pnpm/config@15.8.0
  - @pnpm/cli-utils@0.7.24
  - @pnpm/store-connection-manager@4.1.25
  - @pnpm/find-workspace-packages@4.0.24

## 6.1.25

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-utils@4.2.0
  - @pnpm/get-context@6.2.6
  - @pnpm/lockfile-walker@5.0.11
  - @pnpm/config@15.7.1
  - @pnpm/cli-utils@0.7.23
  - @pnpm/store-connection-manager@4.1.24
  - @pnpm/find-workspace-packages@4.0.23
  - @pnpm/link-bins@7.2.2

## 6.1.24

### Patch Changes

- Updated dependencies [4fa1091c8]
  - @pnpm/config@15.7.0
  - @pnpm/cli-utils@0.7.22
  - @pnpm/link-bins@7.2.2
  - @pnpm/store-connection-manager@4.1.23
  - @pnpm/get-context@6.2.5
  - @pnpm/find-workspace-packages@4.0.22
  - @pnpm/lifecycle@13.1.3

## 6.1.23

### Patch Changes

- Updated dependencies [7334b347b]
- Updated dependencies [e3f4d131c]
  - @pnpm/config@15.6.1
  - @pnpm/lockfile-utils@4.1.0
  - @pnpm/cli-utils@0.7.21
  - @pnpm/store-connection-manager@4.1.22
  - @pnpm/link-bins@7.2.1
  - @pnpm/find-workspace-packages@4.0.21

## 6.1.22

### Patch Changes

- 13210db3c: pnpm rebuild accepts --store-dir
- Updated dependencies [28f000509]
- Updated dependencies [406656f80]
  - @pnpm/config@15.6.0
  - @pnpm/link-bins@7.2.0
  - @pnpm/cli-utils@0.7.20
  - @pnpm/store-connection-manager@4.1.21
  - @pnpm/find-workspace-packages@4.0.20

## 6.1.21

### Patch Changes

- @pnpm/config@15.5.2
- @pnpm/cli-utils@0.7.19
- @pnpm/store-connection-manager@4.1.20
- @pnpm/find-workspace-packages@4.0.19

## 6.1.20

### Patch Changes

- @pnpm/cli-utils@0.7.18
- @pnpm/link-bins@7.1.7
- dependency-path@9.2.3
- @pnpm/find-workspace-packages@4.0.18
- @pnpm/lockfile-utils@4.0.10
- @pnpm/lockfile-walker@5.0.10
- @pnpm/store-connection-manager@4.1.19

## 6.1.19

### Patch Changes

- @pnpm/get-context@6.2.4
- @pnpm/store-connection-manager@4.1.18

## 6.1.18

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- 42c1ea1c0: Update validate-npm-package-name to v4.
- Updated dependencies [5f643f23b]
  - @pnpm/cli-utils@0.7.17
  - @pnpm/config@15.5.1
  - @pnpm/get-context@6.2.3
  - @pnpm/link-bins@7.1.6
  - @pnpm/lockfile-utils@4.0.9
  - @pnpm/lockfile-walker@5.0.9
  - @pnpm/find-workspace-packages@4.0.17
  - @pnpm/store-connection-manager@4.1.17
  - @pnpm/lifecycle@13.1.2

## 6.1.17

### Patch Changes

- Updated dependencies [fc581d371]
  - dependency-path@9.2.2
  - @pnpm/lockfile-utils@4.0.8
  - @pnpm/lockfile-walker@5.0.8
  - @pnpm/store-connection-manager@4.1.16

## 6.1.16

### Patch Changes

- @pnpm/store-connection-manager@4.1.15

## 6.1.15

### Patch Changes

- Updated dependencies [f48d46ef6]
  - @pnpm/config@15.5.0
  - @pnpm/cli-utils@0.7.16
  - @pnpm/store-connection-manager@4.1.14
  - @pnpm/find-workspace-packages@4.0.16

## 6.1.14

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/lockfile-utils@4.0.7
  - @pnpm/lockfile-walker@5.0.7
  - @pnpm/cli-utils@0.7.15
  - @pnpm/config@15.4.1
  - @pnpm/core-loggers@7.0.5
  - dependency-path@9.2.1
  - @pnpm/find-workspace-packages@4.0.15
  - @pnpm/get-context@6.2.2
  - @pnpm/lifecycle@13.1.1
  - @pnpm/link-bins@7.1.5
  - @pnpm/modules-yaml@10.0.5
  - @pnpm/normalize-registries@3.0.5
  - @pnpm/sort-packages@3.0.6
  - @pnpm/store-controller-types@14.0.1
  - @pnpm/store-connection-manager@4.1.13

## 6.1.13

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [c635f9fc1]
- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/types@8.3.0
  - @pnpm/lifecycle@13.1.0
  - dependency-path@9.2.0
  - @pnpm/store-controller-types@14.0.0
  - @pnpm/config@15.4.0
  - @pnpm/cli-utils@0.7.14
  - @pnpm/core-loggers@7.0.4
  - @pnpm/find-workspace-packages@4.0.14
  - @pnpm/get-context@6.2.1
  - @pnpm/link-bins@7.1.4
  - @pnpm/lockfile-utils@4.0.6
  - @pnpm/lockfile-walker@5.0.6
  - @pnpm/modules-yaml@10.0.4
  - @pnpm/normalize-registries@3.0.4
  - @pnpm/sort-packages@3.0.5
  - @pnpm/store-connection-manager@4.1.12

## 6.1.12

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [56cf04cb3]
- Updated dependencies [725636a90]
  - @pnpm/types@8.2.0
  - @pnpm/config@15.3.0
  - @pnpm/get-context@6.2.0
  - dependency-path@9.1.4
  - @pnpm/cli-utils@0.7.13
  - @pnpm/core-loggers@7.0.3
  - @pnpm/find-workspace-packages@4.0.13
  - @pnpm/lifecycle@13.0.5
  - @pnpm/link-bins@7.1.3
  - @pnpm/lockfile-utils@4.0.5
  - @pnpm/lockfile-walker@5.0.5
  - @pnpm/modules-yaml@10.0.3
  - @pnpm/normalize-registries@3.0.3
  - @pnpm/sort-packages@3.0.4
  - @pnpm/store-controller-types@13.0.4
  - @pnpm/store-connection-manager@4.1.11

## 6.1.11

### Patch Changes

- Updated dependencies [25798aad1]
  - @pnpm/config@15.2.1
  - @pnpm/store-connection-manager@4.1.10
  - @pnpm/cli-utils@0.7.12
  - @pnpm/find-workspace-packages@4.0.12

## 6.1.10

### Patch Changes

- Updated dependencies [4d39e4a0c]
- Updated dependencies [bc80631d3]
- Updated dependencies [d5730ba81]
  - @pnpm/types@8.1.0
  - @pnpm/config@15.2.0
  - @pnpm/cli-utils@0.7.11
  - @pnpm/core-loggers@7.0.2
  - dependency-path@9.1.3
  - @pnpm/find-workspace-packages@4.0.11
  - @pnpm/get-context@6.1.3
  - @pnpm/lifecycle@13.0.4
  - @pnpm/link-bins@7.1.2
  - @pnpm/lockfile-utils@4.0.4
  - @pnpm/lockfile-walker@5.0.4
  - @pnpm/modules-yaml@10.0.2
  - @pnpm/normalize-registries@3.0.2
  - @pnpm/sort-packages@3.0.3
  - @pnpm/store-controller-types@13.0.3
  - @pnpm/store-connection-manager@4.1.9

## 6.1.9

### Patch Changes

- Updated dependencies [6756c2b02]
  - @pnpm/store-controller-types@13.0.2
  - @pnpm/cli-utils@0.7.10
  - @pnpm/lifecycle@13.0.3
  - @pnpm/find-workspace-packages@4.0.10
  - @pnpm/store-connection-manager@4.1.8
  - @pnpm/config@15.1.4
  - @pnpm/link-bins@7.1.1

## 6.1.8

### Patch Changes

- Updated dependencies [ae2f845c5]
  - @pnpm/config@15.1.4
  - @pnpm/cli-utils@0.7.9
  - @pnpm/store-connection-manager@4.1.7
  - @pnpm/find-workspace-packages@4.0.9

## 6.1.7

### Patch Changes

- Updated dependencies [05159665d]
  - @pnpm/config@15.1.3
  - @pnpm/cli-utils@0.7.8
  - @pnpm/store-connection-manager@4.1.6
  - @pnpm/find-workspace-packages@4.0.8

## 6.1.6

### Patch Changes

- @pnpm/cli-utils@0.7.7
- @pnpm/find-workspace-packages@4.0.7

## 6.1.5

### Patch Changes

- Updated dependencies [af22c6c4f]
- Updated dependencies [c57695550]
  - @pnpm/config@15.1.2
  - dependency-path@9.1.2
  - @pnpm/cli-utils@0.7.6
  - @pnpm/store-connection-manager@4.1.5
  - @pnpm/lockfile-utils@4.0.3
  - @pnpm/lockfile-walker@5.0.3
  - @pnpm/find-workspace-packages@4.0.6

## 6.1.4

### Patch Changes

- Updated dependencies [52b0576af]
  - @pnpm/cli-utils@0.7.5
  - @pnpm/find-workspace-packages@4.0.5
  - @pnpm/get-context@6.1.2
  - @pnpm/store-connection-manager@4.1.4

## 6.1.3

### Patch Changes

- @pnpm/cli-utils@0.7.4
- @pnpm/store-connection-manager@4.1.3
- @pnpm/find-workspace-packages@4.0.4
- @pnpm/config@15.1.1
- @pnpm/link-bins@7.1.1

## 6.1.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/cli-utils@0.7.3
  - @pnpm/config@15.1.1
  - @pnpm/core-loggers@7.0.1
  - dependency-path@9.1.1
  - @pnpm/find-workspace-packages@4.0.3
  - @pnpm/get-context@6.1.1
  - @pnpm/lifecycle@13.0.2
  - @pnpm/link-bins@7.1.1
  - @pnpm/lockfile-utils@4.0.2
  - @pnpm/lockfile-walker@5.0.2
  - @pnpm/modules-yaml@10.0.1
  - @pnpm/normalize-registries@3.0.1
  - @pnpm/sort-packages@3.0.2
  - @pnpm/store-controller-types@13.0.1
  - @pnpm/store-connection-manager@4.1.2

## 6.1.1

### Patch Changes

- Updated dependencies [e05dcc48a]
  - @pnpm/config@15.1.0
  - @pnpm/cli-utils@0.7.2
  - @pnpm/store-connection-manager@4.1.1
  - @pnpm/find-workspace-packages@4.0.2

## 6.1.0

### Minor Changes

- 8fa95fd86: Path `extraNodePaths` to the bins linker.

### Patch Changes

- 2109f2e8e: Use `@pnpm/graph-sequencer` instead of `graph-sequencer`.
- Updated dependencies [0a70aedb1]
- Updated dependencies [8fa95fd86]
- Updated dependencies [2109f2e8e]
- Updated dependencies [8fa95fd86]
- Updated dependencies [8dac029ef]
- Updated dependencies [688b0eaff]
- Updated dependencies [72b79f55a]
- Updated dependencies [546e644e9]
- Updated dependencies [c6463b9fd]
- Updated dependencies [4bed585e2]
- Updated dependencies [8fa95fd86]
- Updated dependencies [1267e4eff]
  - dependency-path@9.1.0
  - @pnpm/link-bins@7.1.0
  - @pnpm/sort-packages@3.0.1
  - @pnpm/get-context@6.1.0
  - @pnpm/config@15.0.0
  - @pnpm/lockfile-utils@4.0.1
  - @pnpm/store-connection-manager@4.1.0
  - @pnpm/constants@6.1.0
  - @pnpm/lockfile-walker@5.0.1
  - @pnpm/cli-utils@0.7.1
  - @pnpm/lifecycle@13.0.1
  - @pnpm/find-workspace-packages@4.0.1

## 6.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [516859178]
- Updated dependencies [d504dc380]
- Updated dependencies [73d71a2d5]
- Updated dependencies [fa656992c]
- Updated dependencies [faf830b8f]
- Updated dependencies [542014839]
- Updated dependencies [d999a0801]
- Updated dependencies [585e9ca9e]
  - @pnpm/config@14.0.0
  - @pnpm/link-bins@7.0.0
  - @pnpm/types@8.0.0
  - dependency-path@9.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/find-workspace-packages@4.0.0
  - @pnpm/get-context@6.0.0
  - @pnpm/lifecycle@13.0.0
  - @pnpm/lockfile-utils@4.0.0
  - @pnpm/lockfile-walker@5.0.0
  - @pnpm/modules-yaml@10.0.0
  - @pnpm/normalize-registries@3.0.0
  - @pnpm/sort-packages@3.0.0
  - @pnpm/store-connection-manager@4.0.0
  - @pnpm/store-controller-types@13.0.0
  - @pnpm/cli-utils@0.7.0
  - @pnpm/common-cli-options-help@0.9.0

## 5.4.14

### Patch Changes

- Updated dependencies [5c525db13]
  - @pnpm/store-controller-types@12.0.0
  - @pnpm/cli-utils@0.6.50
  - @pnpm/config@13.13.2
  - @pnpm/get-context@5.3.8
  - @pnpm/link-bins@6.2.12
  - @pnpm/store-connection-manager@3.2.10
  - @pnpm/lifecycle@12.1.7
  - @pnpm/find-workspace-packages@3.1.42

## 5.4.13

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/get-context@5.3.7
  - @pnpm/lockfile-utils@3.2.1
  - @pnpm/lockfile-walker@4.0.15
  - @pnpm/cli-utils@0.6.49
  - @pnpm/config@13.13.1
  - @pnpm/core-loggers@6.1.4
  - dependency-path@8.0.11
  - @pnpm/find-workspace-packages@3.1.41
  - @pnpm/lifecycle@12.1.6
  - @pnpm/link-bins@6.2.11
  - @pnpm/modules-yaml@9.1.1
  - @pnpm/normalize-registries@2.0.13
  - @pnpm/sort-packages@2.1.8
  - @pnpm/store-controller-types@11.0.12
  - @pnpm/store-connection-manager@3.2.9

## 5.4.12

### Patch Changes

- @pnpm/store-connection-manager@3.2.8

## 5.4.11

### Patch Changes

- @pnpm/store-connection-manager@3.2.7

## 5.4.10

### Patch Changes

- Updated dependencies [7ae349cd3]
  - @pnpm/lifecycle@12.1.5

## 5.4.9

### Patch Changes

- Updated dependencies [334e5340a]
  - @pnpm/config@13.13.0
  - @pnpm/cli-utils@0.6.48
  - @pnpm/store-connection-manager@3.2.6
  - @pnpm/find-workspace-packages@3.1.40

## 5.4.8

### Patch Changes

- Updated dependencies [b7566b979]
  - @pnpm/config@13.12.0
  - @pnpm/cli-utils@0.6.47
  - @pnpm/store-connection-manager@3.2.5
  - @pnpm/find-workspace-packages@3.1.39

## 5.4.7

### Patch Changes

- Updated dependencies [cdc521cfa]
- Updated dependencies [cdc521cfa]
  - @pnpm/modules-yaml@9.1.0
  - @pnpm/lockfile-utils@3.2.0
  - @pnpm/get-context@5.3.6
  - @pnpm/config@13.11.0
  - @pnpm/link-bins@6.2.10

## 5.4.6

### Patch Changes

- Updated dependencies [fff0e4493]
  - @pnpm/config@13.11.0
  - @pnpm/cli-utils@0.6.46
  - @pnpm/store-connection-manager@3.2.4
  - @pnpm/link-bins@6.2.10
  - @pnpm/find-workspace-packages@3.1.38

## 5.4.5

### Patch Changes

- @pnpm/cli-utils@0.6.45
- @pnpm/find-workspace-packages@3.1.37

## 5.4.4

### Patch Changes

- Updated dependencies [e76151f66]
- Updated dependencies [26cd01b88]
  - @pnpm/config@13.10.0
  - @pnpm/types@7.9.0
  - @pnpm/lifecycle@12.1.4
  - @pnpm/cli-utils@0.6.44
  - @pnpm/store-connection-manager@3.2.3
  - @pnpm/core-loggers@6.1.3
  - dependency-path@8.0.10
  - @pnpm/find-workspace-packages@3.1.36
  - @pnpm/get-context@5.3.5
  - @pnpm/link-bins@6.2.9
  - @pnpm/lockfile-utils@3.1.6
  - @pnpm/lockfile-walker@4.0.14
  - @pnpm/modules-yaml@9.0.11
  - @pnpm/normalize-registries@2.0.12
  - @pnpm/sort-packages@2.1.7
  - @pnpm/store-controller-types@11.0.11

## 5.4.3

### Patch Changes

- @pnpm/cli-utils@0.6.43
- @pnpm/find-workspace-packages@3.1.35

## 5.4.2

### Patch Changes

- @pnpm/store-connection-manager@3.2.2

## 5.4.1

### Patch Changes

- Updated dependencies [8fe8f5e55]
- Updated dependencies [7375396db]
  - @pnpm/config@13.9.0
  - @pnpm/modules-yaml@9.0.10
  - @pnpm/cli-utils@0.6.42
  - @pnpm/store-connection-manager@3.2.1
  - @pnpm/get-context@5.3.4
  - @pnpm/find-workspace-packages@3.1.34
  - @pnpm/link-bins@6.2.8

## 5.4.0

### Minor Changes

- a6cf11cb7: New optional setting added: userConfig. userConfig may contain token helpers.

### Patch Changes

- Updated dependencies [a6cf11cb7]
- Updated dependencies [732d4962f]
- Updated dependencies [a6cf11cb7]
  - @pnpm/store-connection-manager@3.2.0
  - @pnpm/config@13.8.0
  - @pnpm/cli-utils@0.6.41
  - @pnpm/find-workspace-packages@3.1.33

## 5.3.10

### Patch Changes

- Updated dependencies [701ea0746]
- Updated dependencies [b5734a4a7]
  - @pnpm/link-bins@6.2.8
  - @pnpm/types@7.8.0
  - @pnpm/cli-utils@0.6.40
  - @pnpm/config@13.7.2
  - @pnpm/core-loggers@6.1.2
  - dependency-path@8.0.9
  - @pnpm/find-workspace-packages@3.1.32
  - @pnpm/get-context@5.3.3
  - @pnpm/lifecycle@12.1.3
  - @pnpm/lockfile-utils@3.1.5
  - @pnpm/lockfile-walker@4.0.13
  - @pnpm/modules-yaml@9.0.9
  - @pnpm/normalize-registries@2.0.11
  - @pnpm/sort-packages@2.1.6
  - @pnpm/store-controller-types@11.0.10
  - @pnpm/store-connection-manager@3.1.17

## 5.3.9

### Patch Changes

- @pnpm/get-context@5.3.2

## 5.3.8

### Patch Changes

- @pnpm/cli-utils@0.6.39
- @pnpm/find-workspace-packages@3.1.31

## 5.3.7

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/cli-utils@0.6.38
  - @pnpm/config@13.7.1
  - @pnpm/core-loggers@6.1.1
  - dependency-path@8.0.8
  - @pnpm/find-workspace-packages@3.1.30
  - @pnpm/get-context@5.3.1
  - @pnpm/lifecycle@12.1.2
  - @pnpm/link-bins@6.2.7
  - @pnpm/lockfile-utils@3.1.4
  - @pnpm/lockfile-walker@4.0.12
  - @pnpm/modules-yaml@9.0.8
  - @pnpm/normalize-registries@2.0.10
  - @pnpm/sort-packages@2.1.5
  - @pnpm/store-controller-types@11.0.9
  - @pnpm/store-connection-manager@3.1.16

## 5.3.6

### Patch Changes

- Updated dependencies [ba9b2eba1]
- Updated dependencies [30bfca967]
- Updated dependencies [927c4a089]
- Updated dependencies [10a4bd4db]
- Updated dependencies [ba9b2eba1]
- Updated dependencies [25f0fa9fa]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/config@13.7.0
  - @pnpm/normalize-registries@2.0.9
  - @pnpm/common-cli-options-help@0.8.0
  - @pnpm/types@7.7.0
  - @pnpm/get-context@5.3.0
  - @pnpm/lifecycle@12.1.1
  - @pnpm/cli-utils@0.6.37
  - @pnpm/store-connection-manager@3.1.15
  - dependency-path@8.0.7
  - @pnpm/find-workspace-packages@3.1.29
  - @pnpm/link-bins@6.2.6
  - @pnpm/lockfile-utils@3.1.3
  - @pnpm/lockfile-walker@4.0.11
  - @pnpm/modules-yaml@9.0.7
  - @pnpm/sort-packages@2.1.4
  - @pnpm/store-controller-types@11.0.8

## 5.3.5

### Patch Changes

- @pnpm/store-connection-manager@3.1.14

## 5.3.4

### Patch Changes

- Updated dependencies [46aaf7108]
  - @pnpm/config@13.6.1
  - @pnpm/normalize-registries@2.0.8
  - @pnpm/cli-utils@0.6.36
  - @pnpm/store-connection-manager@3.1.13
  - @pnpm/find-workspace-packages@3.1.28
  - @pnpm/get-context@5.2.2

## 5.3.3

### Patch Changes

- Updated dependencies [3cf543fc1]
- Updated dependencies [8a99a01ff]
  - @pnpm/lockfile-utils@3.1.2
  - @pnpm/config@13.6.0
  - @pnpm/cli-utils@0.6.35
  - @pnpm/store-connection-manager@3.1.12
  - @pnpm/find-workspace-packages@3.1.27

## 5.3.2

### Patch Changes

- @pnpm/cli-utils@0.6.34
- @pnpm/store-connection-manager@3.1.11
- @pnpm/find-workspace-packages@3.1.26

## 5.3.1

### Patch Changes

- Updated dependencies [a7ff2d5ce]
  - @pnpm/config@13.5.1
  - @pnpm/normalize-registries@2.0.7
  - @pnpm/cli-utils@0.6.33
  - @pnpm/store-connection-manager@3.1.10
  - @pnpm/find-workspace-packages@3.1.25
  - @pnpm/get-context@5.2.1

## 5.3.0

### Minor Changes

- 002778559: New setting added: `scriptsPrependNodePath`. This setting can be `true`, `false`, or `warn-only`.
  When `true`, the path to the `node` executable with which pnpm executed is prepended to the `PATH` of the scripts.
  When `warn-only`, pnpm will print a warning if the scripts run with a `node` binary that differs from the `node` binary executing the pnpm CLI.

### Patch Changes

- Updated dependencies [002778559]
  - @pnpm/config@13.5.0
  - @pnpm/lifecycle@12.1.0
  - @pnpm/cli-utils@0.6.32
  - @pnpm/store-connection-manager@3.1.9
  - @pnpm/find-workspace-packages@3.1.24

## 5.2.6

### Patch Changes

- @pnpm/store-connection-manager@3.1.8

## 5.2.5

### Patch Changes

- @pnpm/store-connection-manager@3.1.7

## 5.2.4

### Patch Changes

- Updated dependencies [1647d8e2f]
- Updated dependencies [bb0f8bc16]
  - @pnpm/store-connection-manager@3.1.6
  - @pnpm/link-bins@6.2.5
  - @pnpm/cli-utils@0.6.31
  - @pnpm/find-workspace-packages@3.1.23

## 5.2.3

### Patch Changes

- Updated dependencies [302ae4f6f]
- Updated dependencies [fa03cbdc8]
  - @pnpm/get-context@5.2.0
  - @pnpm/types@7.6.0
  - @pnpm/lifecycle@12.0.2
  - @pnpm/config@13.4.2
  - @pnpm/cli-utils@0.6.30
  - @pnpm/core-loggers@6.0.6
  - dependency-path@8.0.6
  - @pnpm/find-workspace-packages@3.1.22
  - @pnpm/link-bins@6.2.4
  - @pnpm/lockfile-utils@3.1.1
  - @pnpm/lockfile-walker@4.0.10
  - @pnpm/modules-yaml@9.0.6
  - @pnpm/normalize-registries@2.0.6
  - @pnpm/sort-packages@2.1.3
  - @pnpm/store-controller-types@11.0.7
  - @pnpm/store-connection-manager@3.1.5

## 5.2.2

### Patch Changes

- Updated dependencies [5b90ab98f]
  - @pnpm/lifecycle@12.0.1

## 5.2.1

### Patch Changes

- @pnpm/store-connection-manager@3.1.4

## 5.2.0

### Minor Changes

- 4ab87844a: Injected dependencies should be relinked after they are rebuilt.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [37dcfceeb]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/lifecycle@12.0.0
  - @pnpm/lockfile-utils@3.1.0
  - @pnpm/cli-utils@0.6.29
  - @pnpm/config@13.4.1
  - @pnpm/core-loggers@6.0.5
  - dependency-path@8.0.5
  - @pnpm/find-workspace-packages@3.1.21
  - @pnpm/get-context@5.1.6
  - @pnpm/link-bins@6.2.3
  - @pnpm/lockfile-walker@4.0.9
  - @pnpm/modules-yaml@9.0.5
  - @pnpm/normalize-registries@2.0.5
  - @pnpm/sort-packages@2.1.2
  - @pnpm/store-controller-types@11.0.6
  - @pnpm/store-connection-manager@3.1.3

## 5.1.5

### Patch Changes

- Updated dependencies [a916accec]
  - @pnpm/link-bins@6.2.2
  - @pnpm/store-connection-manager@3.1.2

## 5.1.4

### Patch Changes

- Updated dependencies [b6d74c545]
  - @pnpm/config@13.4.0
  - @pnpm/cli-utils@0.6.28
  - @pnpm/store-connection-manager@3.1.1
  - @pnpm/find-workspace-packages@3.1.20

## 5.1.3

### Patch Changes

- Updated dependencies [6375cdce0]
- Updated dependencies [bd7bcdbe8]
- Updated dependencies [bd7bcdbe8]
  - @pnpm/link-bins@6.2.1
  - @pnpm/store-connection-manager@3.1.0
  - @pnpm/config@13.3.0
  - @pnpm/cli-utils@0.6.27
  - @pnpm/find-workspace-packages@3.1.19

## 5.1.2

### Patch Changes

- Updated dependencies [5ee3b2dc7]
  - @pnpm/config@13.2.0
  - @pnpm/cli-utils@0.6.26
  - @pnpm/store-connection-manager@3.0.20
  - @pnpm/find-workspace-packages@3.1.18

## 5.1.1

### Patch Changes

- @pnpm/cli-utils@0.6.25
- @pnpm/find-workspace-packages@3.1.17

## 5.1.0

### Minor Changes

- 4027a3c69: New optional option added: hooks.

### Patch Changes

- Updated dependencies [4027a3c69]
- Updated dependencies [1efaaf706]
  - @pnpm/config@13.1.0
  - @pnpm/common-cli-options-help@0.7.1
  - @pnpm/cli-utils@0.6.24
  - @pnpm/store-connection-manager@3.0.19
  - @pnpm/find-workspace-packages@3.1.16

## 5.0.19

### Patch Changes

- Updated dependencies [4a4d42d8f]
  - @pnpm/lifecycle@11.0.5
  - @pnpm/store-connection-manager@3.0.18

## 5.0.18

### Patch Changes

- Updated dependencies [0d4a7c69e]
- Updated dependencies [fe5688dc0]
- Updated dependencies [c7081cbb4]
- Updated dependencies [c7081cbb4]
  - @pnpm/link-bins@6.2.0
  - @pnpm/common-cli-options-help@0.7.0
  - @pnpm/config@13.0.0
  - @pnpm/cli-utils@0.6.23
  - @pnpm/store-connection-manager@3.0.17
  - @pnpm/find-workspace-packages@3.1.15

## 5.0.17

### Patch Changes

- Updated dependencies [83e23601e]
- Updated dependencies [553a5d840]
- Updated dependencies [d62259d67]
  - @pnpm/link-bins@6.1.0
  - @pnpm/config@12.6.0
  - @pnpm/cli-utils@0.6.22
  - @pnpm/store-connection-manager@3.0.16
  - @pnpm/find-workspace-packages@3.1.14

## 5.0.16

### Patch Changes

- @pnpm/store-connection-manager@3.0.15

## 5.0.15

### Patch Changes

- Updated dependencies [6681fdcbc]
  - @pnpm/config@12.5.0
  - @pnpm/cli-utils@0.6.21
  - @pnpm/store-connection-manager@3.0.14
  - @pnpm/find-workspace-packages@3.1.13

## 5.0.14

### Patch Changes

- @pnpm/link-bins@6.0.8
- @pnpm/cli-utils@0.6.20
- @pnpm/store-connection-manager@3.0.13
- @pnpm/find-workspace-packages@3.1.12

## 5.0.13

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9
  - @pnpm/cli-utils@0.6.19
  - @pnpm/store-connection-manager@3.0.12
  - @pnpm/find-workspace-packages@3.1.11

## 5.0.12

### Patch Changes

- @pnpm/config@12.4.8
- @pnpm/cli-utils@0.6.18
- @pnpm/store-connection-manager@3.0.11
- @pnpm/find-workspace-packages@3.1.10

## 5.0.11

### Patch Changes

- @pnpm/store-connection-manager@3.0.10

## 5.0.10

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7
  - @pnpm/cli-utils@0.6.17
  - @pnpm/store-connection-manager@3.0.9
  - @pnpm/find-workspace-packages@3.1.9

## 5.0.9

### Patch Changes

- @pnpm/store-connection-manager@3.0.8

## 5.0.8

### Patch Changes

- @pnpm/store-connection-manager@3.0.7

## 5.0.7

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6
  - @pnpm/cli-utils@0.6.16
  - @pnpm/store-connection-manager@3.0.6
  - @pnpm/find-workspace-packages@3.1.8

## 5.0.6

### Patch Changes

- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5
  - @pnpm/cli-utils@0.6.15
  - @pnpm/store-connection-manager@3.0.5
  - @pnpm/find-workspace-packages@3.1.7

## 5.0.5

### Patch Changes

- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4
  - @pnpm/link-bins@6.0.7
  - @pnpm/cli-utils@0.6.14
  - @pnpm/store-connection-manager@3.0.4
  - @pnpm/find-workspace-packages@3.1.6

## 5.0.4

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/cli-utils@0.6.13
  - @pnpm/config@12.4.3
  - @pnpm/core-loggers@6.0.4
  - dependency-path@8.0.4
  - @pnpm/find-workspace-packages@3.1.5
  - @pnpm/get-context@5.1.5
  - @pnpm/lifecycle@11.0.4
  - @pnpm/link-bins@6.0.6
  - @pnpm/lockfile-utils@3.0.8
  - @pnpm/lockfile-walker@4.0.8
  - @pnpm/modules-yaml@9.0.4
  - @pnpm/normalize-registries@2.0.4
  - @pnpm/sort-packages@2.1.1
  - @pnpm/store-controller-types@11.0.5
  - @pnpm/store-connection-manager@3.0.3

## 5.0.3

### Patch Changes

- Updated dependencies [7af16a011]
- Updated dependencies [73c1f802e]
  - @pnpm/lifecycle@11.0.3
  - @pnpm/config@12.4.2
  - @pnpm/cli-utils@0.6.12
  - @pnpm/store-connection-manager@3.0.2
  - @pnpm/find-workspace-packages@3.1.4

## 5.0.2

### Patch Changes

- @pnpm/cli-utils@0.6.11
- @pnpm/find-workspace-packages@3.1.3

## 5.0.1

### Patch Changes

- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1
  - @pnpm/cli-utils@0.6.10
  - @pnpm/store-connection-manager@3.0.1
  - @pnpm/find-workspace-packages@3.1.2

## 5.0.0

### Major Changes

- 691f64713: New required option added: cacheDir.

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [691f64713]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0
  - @pnpm/store-connection-manager@3.0.0
  - @pnpm/cli-utils@0.6.9
  - @pnpm/find-workspace-packages@3.1.1

## 4.0.12

### Patch Changes

- Updated dependencies [a5bde0aa2]
  - @pnpm/find-workspace-packages@3.1.0

## 4.0.11

### Patch Changes

- Updated dependencies [1442f8786]
- Updated dependencies [8e76690f4]
  - @pnpm/sort-packages@2.1.0
  - @pnpm/types@7.3.0
  - @pnpm/get-context@5.1.4
  - @pnpm/cli-utils@0.6.8
  - @pnpm/config@12.3.3
  - @pnpm/core-loggers@6.0.3
  - dependency-path@8.0.3
  - @pnpm/find-workspace-packages@3.0.8
  - @pnpm/lifecycle@11.0.2
  - @pnpm/link-bins@6.0.5
  - @pnpm/lockfile-utils@3.0.7
  - @pnpm/lockfile-walker@4.0.7
  - @pnpm/modules-yaml@9.0.3
  - @pnpm/normalize-registries@2.0.3
  - @pnpm/store-controller-types@11.0.4
  - @pnpm/store-connection-manager@2.1.11

## 4.0.10

### Patch Changes

- Updated dependencies [6c418943c]
  - dependency-path@8.0.2
  - @pnpm/lockfile-utils@3.0.6
  - @pnpm/lockfile-walker@4.0.6
  - @pnpm/store-connection-manager@2.1.10

## 4.0.9

### Patch Changes

- @pnpm/store-connection-manager@2.1.9

## 4.0.8

### Patch Changes

- @pnpm/get-context@5.1.3

## 4.0.7

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/store-connection-manager@2.1.8
  - @pnpm/cli-utils@0.6.7
  - @pnpm/config@12.3.2
  - @pnpm/core-loggers@6.0.2
  - dependency-path@8.0.1
  - @pnpm/find-workspace-packages@3.0.7
  - @pnpm/get-context@5.1.2
  - @pnpm/lifecycle@11.0.1
  - @pnpm/link-bins@6.0.4
  - @pnpm/lockfile-utils@3.0.5
  - @pnpm/lockfile-walker@4.0.5
  - @pnpm/modules-yaml@9.0.2
  - @pnpm/normalize-registries@2.0.2
  - @pnpm/sort-packages@2.0.2
  - @pnpm/store-controller-types@11.0.3

## 4.0.6

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1
  - @pnpm/get-context@5.1.1
  - @pnpm/link-bins@6.0.3
  - @pnpm/lockfile-utils@3.0.4
  - @pnpm/lockfile-walker@4.0.4
  - @pnpm/cli-utils@0.6.6
  - @pnpm/store-connection-manager@2.1.7
  - @pnpm/find-workspace-packages@3.0.6

## 4.0.5

### Patch Changes

- @pnpm/store-connection-manager@2.1.6

## 4.0.4

### Patch Changes

- Updated dependencies [84ec82e05]
- Updated dependencies [c2a71e4fd]
- Updated dependencies [84ec82e05]
  - @pnpm/config@12.3.0
  - @pnpm/common-cli-options-help@0.6.0
  - @pnpm/cli-utils@0.6.5
  - @pnpm/store-connection-manager@2.1.5
  - @pnpm/find-workspace-packages@3.0.5

## 4.0.3

### Patch Changes

- Updated dependencies [20e2f235d]
  - dependency-path@8.0.0
  - @pnpm/lockfile-utils@3.0.3
  - @pnpm/lockfile-walker@4.0.3
  - @pnpm/cli-utils@0.6.4
  - @pnpm/find-workspace-packages@3.0.4
  - @pnpm/store-connection-manager@2.1.4

## 4.0.2

### Patch Changes

- @pnpm/cli-utils@0.6.3
- @pnpm/link-bins@6.0.2
- @pnpm/store-connection-manager@2.1.3
- @pnpm/find-workspace-packages@3.0.3
- @pnpm/config@12.2.0

## 4.0.1

### Patch Changes

- @pnpm/store-connection-manager@2.1.2

## 4.0.0

### Major Changes

- e6a2654a2: `prepare` scripts of Git-hosted packages are not executed (they are executed during fetching by `@pnpm/git-fetcher`).

### Patch Changes

- Updated dependencies [e6a2654a2]
  - @pnpm/lifecycle@11.0.0
  - @pnpm/store-controller-types@11.0.2
  - @pnpm/store-connection-manager@2.1.1
  - @pnpm/config@12.2.0

## 3.0.4

### Patch Changes

- Updated dependencies [05baaa6e7]
- Updated dependencies [dfdf669e6]
- Updated dependencies [97c64bae4]
- Updated dependencies [97c64bae4]
  - @pnpm/config@12.2.0
  - @pnpm/store-connection-manager@2.1.0
  - @pnpm/common-cli-options-help@0.5.0
  - @pnpm/get-context@5.1.0
  - @pnpm/types@7.1.0
  - @pnpm/cli-utils@0.6.2
  - @pnpm/link-bins@6.0.1
  - @pnpm/core-loggers@6.0.1
  - dependency-path@7.0.1
  - @pnpm/find-workspace-packages@3.0.2
  - @pnpm/lifecycle@10.0.1
  - @pnpm/lockfile-utils@3.0.2
  - @pnpm/lockfile-walker@4.0.2
  - @pnpm/modules-yaml@9.0.1
  - @pnpm/normalize-registries@2.0.1
  - @pnpm/sort-packages@2.0.1
  - @pnpm/store-controller-types@11.0.1

## 3.0.3

### Patch Changes

- Updated dependencies [ba5231ccf]
  - @pnpm/config@12.1.0
  - @pnpm/cli-utils@0.6.1
  - @pnpm/store-connection-manager@2.0.3
  - @pnpm/find-workspace-packages@3.0.1

## 3.0.2

### Patch Changes

- @pnpm/store-connection-manager@2.0.2
- @pnpm/config@12.0.0

## 3.0.1

### Patch Changes

- Updated dependencies [9ceab68f0]
  - dependency-path@7.0.0
  - @pnpm/lockfile-utils@3.0.1
  - @pnpm/lockfile-walker@4.0.1
  - @pnpm/store-connection-manager@2.0.1

## 3.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- 78470a32d: `prunedAt` is set for the modules meta file.
- Updated dependencies [6871d74b2]
- Updated dependencies [06c6c9959]
- Updated dependencies [97b986fbc]
- Updated dependencies [90487a3a8]
- Updated dependencies [78470a32d]
- Updated dependencies [aed712455]
- Updated dependencies [e4efddbd2]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [aed712455]
- Updated dependencies [7adc6e875]
- Updated dependencies [78470a32d]
  - @pnpm/constants@5.0.0
  - @pnpm/link-bins@6.0.0
  - @pnpm/cli-utils@0.6.0
  - @pnpm/common-cli-options-help@0.4.0
  - @pnpm/config@12.0.0
  - @pnpm/core-loggers@6.0.0
  - dependency-path@6.0.0
  - @pnpm/find-workspace-packages@3.0.0
  - @pnpm/get-context@5.0.0
  - @pnpm/lifecycle@10.0.0
  - @pnpm/lockfile-utils@3.0.0
  - @pnpm/lockfile-walker@4.0.0
  - @pnpm/modules-yaml@9.0.0
  - @pnpm/normalize-registries@2.0.0
  - @pnpm/sort-packages@2.0.0
  - @pnpm/store-connection-manager@1.1.0
  - @pnpm/store-controller-types@11.0.0
  - @pnpm/types@7.0.0

## 2.2.34

### Patch Changes

- Updated dependencies [4f1ce907a]
  - @pnpm/config@11.14.2
  - @pnpm/cli-utils@0.5.4
  - @pnpm/store-connection-manager@1.0.4
  - @pnpm/find-workspace-packages@2.3.42

## 2.2.33

### Patch Changes

- Updated dependencies [d853fb14a]
- Updated dependencies [d853fb14a]
- Updated dependencies [4b3852c39]
  - @pnpm/lifecycle@9.6.5
  - @pnpm/link-bins@5.3.25
  - @pnpm/config@11.14.1
  - @pnpm/cli-utils@0.5.3
  - @pnpm/store-connection-manager@1.0.3
  - @pnpm/find-workspace-packages@2.3.41

## 2.2.32

### Patch Changes

- Updated dependencies [6350a3381]
  - @pnpm/link-bins@5.3.24
  - @pnpm/store-connection-manager@1.0.2

## 2.2.31

### Patch Changes

- @pnpm/store-connection-manager@1.0.1

## 2.2.30

### Patch Changes

- Updated dependencies [8d1dfa89c]
  - @pnpm/store-connection-manager@1.0.0
  - @pnpm/store-controller-types@10.0.0
  - @pnpm/config@11.14.0
  - @pnpm/cli-utils@0.5.2
  - @pnpm/find-workspace-packages@2.3.40

## 2.2.29

### Patch Changes

- Updated dependencies [3be2b1773]
  - @pnpm/cli-utils@0.5.1
  - @pnpm/find-workspace-packages@2.3.39

## 2.2.28

### Patch Changes

- Updated dependencies [51e1456dd]
  - @pnpm/get-context@4.0.0

## 2.2.27

### Patch Changes

- Updated dependencies [27a40321c]
  - @pnpm/get-context@3.3.6
  - @pnpm/store-connection-manager@0.3.64

## 2.2.26

### Patch Changes

- Updated dependencies [a78e5c47f]
  - @pnpm/link-bins@5.3.23

## 2.2.25

### Patch Changes

- Updated dependencies [a5e9d903c]
- Updated dependencies [cb040ae18]
  - @pnpm/common-cli-options-help@0.3.1
  - @pnpm/cli-utils@0.5.0
  - @pnpm/config@11.14.0
  - @pnpm/find-workspace-packages@2.3.38
  - @pnpm/store-connection-manager@0.3.63

## 2.2.24

### Patch Changes

- Updated dependencies [c4cc62506]
  - @pnpm/config@11.13.0
  - @pnpm/link-bins@5.3.22
  - @pnpm/cli-utils@0.4.51
  - @pnpm/store-connection-manager@0.3.62
  - @pnpm/find-workspace-packages@2.3.37

## 2.2.23

### Patch Changes

- Updated dependencies [bff84dbca]
  - @pnpm/config@11.12.1
  - @pnpm/cli-utils@0.4.50
  - @pnpm/store-connection-manager@0.3.61
  - @pnpm/find-workspace-packages@2.3.36

## 2.2.22

### Patch Changes

- @pnpm/cli-utils@0.4.49
- @pnpm/find-workspace-packages@2.3.35

## 2.2.21

### Patch Changes

- Updated dependencies [43de80034]
  - @pnpm/store-connection-manager@0.3.60
  - @pnpm/cli-utils@0.4.48
  - @pnpm/find-workspace-packages@2.3.34

## 2.2.20

### Patch Changes

- Updated dependencies [9a9bc67d2]
  - @pnpm/lifecycle@9.6.4

## 2.2.19

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [548f28df9]
- Updated dependencies [548f28df9]
  - @pnpm/types@6.4.0
  - @pnpm/cli-utils@0.4.47
  - @pnpm/config@11.12.0
  - @pnpm/get-context@3.3.5
  - @pnpm/lockfile-utils@2.0.22
  - @pnpm/lockfile-walker@3.0.9
  - @pnpm/core-loggers@5.0.3
  - dependency-path@5.1.1
  - @pnpm/find-workspace-packages@2.3.33
  - @pnpm/lifecycle@9.6.3
  - @pnpm/link-bins@5.3.21
  - @pnpm/modules-yaml@8.0.6
  - @pnpm/normalize-registries@1.0.6
  - @pnpm/sort-packages@1.0.16
  - @pnpm/store-controller-types@9.2.1
  - @pnpm/store-connection-manager@0.3.59

## 2.2.18

### Patch Changes

- @pnpm/config@11.11.1
- @pnpm/cli-utils@0.4.46
- @pnpm/store-connection-manager@0.3.58
- @pnpm/find-workspace-packages@2.3.32

## 2.2.17

### Patch Changes

- @pnpm/get-context@3.3.4

## 2.2.16

### Patch Changes

- Updated dependencies [f40bc5927]
  - @pnpm/config@11.11.0
  - @pnpm/get-context@3.3.3
  - @pnpm/cli-utils@0.4.45
  - @pnpm/store-connection-manager@0.3.57
  - @pnpm/find-workspace-packages@2.3.31

## 2.2.15

### Patch Changes

- Updated dependencies [e27dcf0dc]
- Updated dependencies [425c7547d]
  - dependency-path@5.1.0
  - @pnpm/config@11.10.2
  - @pnpm/lockfile-utils@2.0.21
  - @pnpm/lockfile-walker@3.0.8
  - @pnpm/cli-utils@0.4.44
  - @pnpm/store-connection-manager@0.3.56
  - @pnpm/find-workspace-packages@2.3.30

## 2.2.14

### Patch Changes

- Updated dependencies [ea09da716]
  - @pnpm/config@11.10.1
  - @pnpm/cli-utils@0.4.43
  - @pnpm/store-connection-manager@0.3.55
  - @pnpm/find-workspace-packages@2.3.29

## 2.2.13

### Patch Changes

- Updated dependencies [1ec47db33]
- Updated dependencies [a8656b42f]
  - @pnpm/common-cli-options-help@0.3.0
  - @pnpm/config@11.10.0
  - @pnpm/cli-utils@0.4.42
  - @pnpm/store-connection-manager@0.3.54
  - @pnpm/find-workspace-packages@2.3.28

## 2.2.12

### Patch Changes

- Updated dependencies [041537bc3]
  - @pnpm/config@11.9.1
  - @pnpm/cli-utils@0.4.41
  - @pnpm/store-connection-manager@0.3.53
  - @pnpm/find-workspace-packages@2.3.27

## 2.2.11

### Patch Changes

- @pnpm/store-connection-manager@0.3.52

## 2.2.10

### Patch Changes

- Updated dependencies [dc5a0a102]
  - @pnpm/store-connection-manager@0.3.51
  - @pnpm/get-context@3.3.2

## 2.2.9

### Patch Changes

- @pnpm/store-connection-manager@0.3.50

## 2.2.8

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0
  - @pnpm/store-controller-types@9.2.0
  - @pnpm/cli-utils@0.4.40
  - @pnpm/store-connection-manager@0.3.49
  - @pnpm/lockfile-utils@2.0.20
  - @pnpm/find-workspace-packages@2.3.26

## 2.2.7

### Patch Changes

- Updated dependencies [fcc1c7100]
  - @pnpm/config@11.8.0
  - @pnpm/cli-utils@0.4.39
  - @pnpm/store-connection-manager@0.3.48
  - @pnpm/find-workspace-packages@2.3.25

## 2.2.6

### Patch Changes

- @pnpm/cli-utils@0.4.38
- @pnpm/config@11.7.2
- @pnpm/get-context@3.3.1
- @pnpm/link-bins@5.3.20
- @pnpm/store-connection-manager@0.3.47
- @pnpm/find-workspace-packages@2.3.24
- @pnpm/lifecycle@9.6.2

## 2.2.5

### Patch Changes

- Updated dependencies [3776b5a52]
  - @pnpm/get-context@3.3.0

## 2.2.4

### Patch Changes

- Updated dependencies [09492b7b4]
  - @pnpm/modules-yaml@8.0.5
  - @pnpm/get-context@3.2.11
  - @pnpm/store-connection-manager@0.3.46
  - @pnpm/cli-utils@0.4.37
  - @pnpm/link-bins@5.3.19
  - @pnpm/find-workspace-packages@2.3.23

## 2.2.3

### Patch Changes

- Updated dependencies [39142e2ad]
  - dependency-path@5.0.6
  - @pnpm/lockfile-utils@2.0.19
  - @pnpm/lockfile-walker@3.0.7
  - @pnpm/get-context@3.2.10
  - @pnpm/cli-utils@0.4.36
  - @pnpm/link-bins@5.3.18
  - @pnpm/store-connection-manager@0.3.45
  - @pnpm/find-workspace-packages@2.3.22

## 2.2.2

### Patch Changes

- @pnpm/store-connection-manager@0.3.44

## 2.2.1

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/lockfile-utils@2.0.18
  - @pnpm/lockfile-walker@3.0.6
  - @pnpm/cli-utils@0.4.35
  - @pnpm/config@11.7.1
  - @pnpm/core-loggers@5.0.2
  - dependency-path@5.0.5
  - @pnpm/find-workspace-packages@2.3.21
  - @pnpm/get-context@3.2.9
  - @pnpm/lifecycle@9.6.1
  - @pnpm/link-bins@5.3.17
  - @pnpm/modules-yaml@8.0.4
  - @pnpm/normalize-registries@1.0.5
  - @pnpm/sort-packages@1.0.15
  - @pnpm/store-controller-types@9.1.2
  - @pnpm/store-connection-manager@0.3.43

## 2.2.0

### Minor Changes

- 50b360ec1: A new option added for specifying the shell to use, when running scripts: scriptShell.

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0
  - @pnpm/lifecycle@9.6.0
  - @pnpm/cli-utils@0.4.34
  - @pnpm/store-connection-manager@0.3.42
  - @pnpm/find-workspace-packages@2.3.20

## 2.1.6

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - @pnpm/lockfile-utils@2.0.17
  - @pnpm/lockfile-walker@3.0.5
  - @pnpm/cli-utils@0.4.33
  - @pnpm/config@11.6.1
  - @pnpm/core-loggers@5.0.1
  - dependency-path@5.0.4
  - @pnpm/find-workspace-packages@2.3.19
  - @pnpm/get-context@3.2.8
  - @pnpm/lifecycle@9.5.1
  - @pnpm/link-bins@5.3.16
  - @pnpm/modules-yaml@8.0.3
  - @pnpm/normalize-registries@1.0.4
  - @pnpm/sort-packages@1.0.14
  - @pnpm/store-controller-types@9.1.1
  - @pnpm/store-connection-manager@0.3.41

## 2.1.5

### Patch Changes

- Updated dependencies [fb863fae4]
  - @pnpm/link-bins@5.3.15

## 2.1.4

### Patch Changes

- 3a83db407: Update mem to v8.
- Updated dependencies [f591fdeeb]
- Updated dependencies [f591fdeeb]
- Updated dependencies [f591fdeeb]
  - @pnpm/config@11.6.0
  - @pnpm/lifecycle@9.5.0
  - @pnpm/cli-utils@0.4.32
  - @pnpm/store-connection-manager@0.3.40
  - @pnpm/find-workspace-packages@2.3.18

## 2.1.3

### Patch Changes

- Updated dependencies [51311d3ba]
  - @pnpm/link-bins@5.3.14
  - @pnpm/cli-utils@0.4.31
  - @pnpm/find-workspace-packages@2.3.17
  - @pnpm/store-connection-manager@0.3.39

## 2.1.2

### Patch Changes

- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0
  - @pnpm/cli-utils@0.4.30
  - @pnpm/store-connection-manager@0.3.38
  - @pnpm/find-workspace-packages@2.3.16

## 2.1.1

### Patch Changes

- Updated dependencies [203e65ac8]
  - @pnpm/lifecycle@9.4.0
  - @pnpm/store-connection-manager@0.3.37

## 2.1.0

### Minor Changes

- 23cf3c88b: New option added: `shellEmulator`.

### Patch Changes

- Updated dependencies [23cf3c88b]
- Updated dependencies [ac3042858]
  - @pnpm/config@11.4.0
  - @pnpm/lifecycle@9.3.0
  - @pnpm/get-context@3.2.7
  - @pnpm/cli-utils@0.4.29
  - @pnpm/store-connection-manager@0.3.36
  - @pnpm/find-workspace-packages@2.3.15

## 2.0.41

### Patch Changes

- Updated dependencies [0a6544043]
  - @pnpm/store-controller-types@9.1.0
  - @pnpm/store-connection-manager@0.3.35

## 2.0.40

### Patch Changes

- @pnpm/store-connection-manager@0.3.34

## 2.0.39

### Patch Changes

- Updated dependencies [767212f4e]
- Updated dependencies [092f8dd83]
- Updated dependencies [092f8dd83]
  - @pnpm/config@11.3.0
  - @pnpm/common-cli-options-help@0.2.0
  - @pnpm/store-connection-manager@0.3.33
  - @pnpm/cli-utils@0.4.28
  - @pnpm/find-workspace-packages@2.3.14

## 2.0.38

### Patch Changes

- Updated dependencies [86cd72de3]
- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0
  - @pnpm/store-controller-types@9.0.0
  - @pnpm/get-context@3.2.6
  - @pnpm/lifecycle@9.2.5
  - @pnpm/store-connection-manager@0.3.32
  - @pnpm/cli-utils@0.4.27
  - @pnpm/find-workspace-packages@2.3.13

## 2.0.37

### Patch Changes

- 968c26470: Report an info log instead of a warning when some binaries cannot be linked.
  - @pnpm/store-connection-manager@0.3.31

## 2.0.36

### Patch Changes

- @pnpm/cli-utils@0.4.26
- @pnpm/find-workspace-packages@2.3.12

## 2.0.35

### Patch Changes

- @pnpm/store-connection-manager@0.3.30

## 2.0.34

### Patch Changes

- Updated dependencies [9f1a29ff9]
  - @pnpm/config@11.2.7
  - @pnpm/cli-utils@0.4.25
  - @pnpm/get-context@3.2.5
  - @pnpm/link-bins@5.3.13
  - @pnpm/store-connection-manager@0.3.29
  - @pnpm/find-workspace-packages@2.3.11
  - @pnpm/lifecycle@9.2.4

## 2.0.33

### Patch Changes

- Updated dependencies [ac0d3e122]
  - @pnpm/config@11.2.6
  - @pnpm/cli-utils@0.4.24
  - @pnpm/store-connection-manager@0.3.28
  - @pnpm/find-workspace-packages@2.3.10

## 2.0.32

### Patch Changes

- Updated dependencies [972864e0d]
- Updated dependencies [972864e0d]
  - @pnpm/config@11.2.5
  - @pnpm/get-context@3.2.4
  - @pnpm/store-connection-manager@0.3.27
  - @pnpm/lifecycle@9.2.3
  - @pnpm/link-bins@5.3.12
  - @pnpm/cli-utils@0.4.23
  - @pnpm/find-workspace-packages@2.3.9

## 2.0.31

### Patch Changes

- Updated dependencies [51086e6e4]
  - @pnpm/get-context@3.2.3
  - @pnpm/cli-utils@0.4.22
  - @pnpm/config@11.2.4
  - @pnpm/link-bins@5.3.11
  - @pnpm/store-connection-manager@0.3.26
  - @pnpm/find-workspace-packages@2.3.8

## 2.0.30

### Patch Changes

- Updated dependencies [13c18e397]
  - @pnpm/config@11.2.3
  - @pnpm/cli-utils@0.4.21
  - @pnpm/store-connection-manager@0.3.25
  - @pnpm/find-workspace-packages@2.3.7

## 2.0.29

### Patch Changes

- Updated dependencies [3f6d35997]
  - @pnpm/config@11.2.2
  - @pnpm/cli-utils@0.4.20
  - @pnpm/store-connection-manager@0.3.24
  - @pnpm/find-workspace-packages@2.3.6

## 2.0.28

### Patch Changes

- @pnpm/cli-utils@0.4.19
- @pnpm/link-bins@5.3.10
- @pnpm/find-workspace-packages@2.3.5
- @pnpm/store-connection-manager@0.3.23

## 2.0.27

### Patch Changes

- @pnpm/cli-utils@0.4.18
- @pnpm/link-bins@5.3.9
- @pnpm/find-workspace-packages@2.3.4
- @pnpm/store-connection-manager@0.3.22

## 2.0.26

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [1140ef721]
- Updated dependencies [a2ef8084f]
  - @pnpm/lockfile-utils@2.0.16
  - @pnpm/config@11.2.1
  - dependency-path@5.0.3
  - @pnpm/find-workspace-packages@2.3.3
  - @pnpm/get-context@3.2.2
  - @pnpm/lifecycle@9.2.2
  - @pnpm/lockfile-walker@3.0.4
  - @pnpm/modules-yaml@8.0.2
  - @pnpm/cli-utils@0.4.17
  - @pnpm/store-connection-manager@0.3.21
  - @pnpm/link-bins@5.3.8

## 2.0.25

### Patch Changes

- Updated dependencies [25b425ca2]
  - @pnpm/get-context@3.2.1

## 2.0.24

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/cli-utils@0.4.16
  - @pnpm/config@11.2.0
  - @pnpm/find-workspace-packages@2.3.2
  - @pnpm/store-connection-manager@0.3.20

## 2.0.23

### Patch Changes

- Updated dependencies [a01626668]
  - @pnpm/get-context@3.2.0

## 2.0.22

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0
  - @pnpm/get-context@3.1.0
  - @pnpm/lifecycle@9.2.1
  - @pnpm/store-connection-manager@0.3.19
  - @pnpm/cli-utils@0.4.15
  - @pnpm/find-workspace-packages@2.3.1

## 2.0.21

### Patch Changes

- Updated dependencies [faae9a93c]
- Updated dependencies [65b4d07ca]
- Updated dependencies [ab3b8f51d]
- Updated dependencies [7b98d16c8]
  - @pnpm/find-workspace-packages@2.3.0
  - @pnpm/config@11.1.0
  - @pnpm/store-connection-manager@0.3.18
  - @pnpm/cli-utils@0.4.14

## 2.0.20

### Patch Changes

- Updated dependencies [d9310c034]
  - @pnpm/store-connection-manager@0.3.17

## 2.0.19

### Patch Changes

- Updated dependencies [76aaead32]
  - @pnpm/lifecycle@9.2.0

## 2.0.18

### Patch Changes

- @pnpm/config@11.0.1
- @pnpm/cli-utils@0.4.13
- @pnpm/store-connection-manager@0.3.16
- @pnpm/find-workspace-packages@2.2.11

## 2.0.17

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [915828b46]
  - @pnpm/config@11.0.0
  - @pnpm/cli-utils@0.4.12
  - @pnpm/store-connection-manager@0.3.15
  - @pnpm/find-workspace-packages@2.2.10

## 2.0.16

### Patch Changes

- @pnpm/store-connection-manager@0.3.14

## 2.0.15

### Patch Changes

- @pnpm/config@10.0.1
- @pnpm/cli-utils@0.4.11
- @pnpm/store-connection-manager@0.3.13
- @pnpm/find-workspace-packages@2.2.9

## 2.0.14

### Patch Changes

- 220896511: Remove common-tags from dependencies.
- Updated dependencies [db17f6f7b]
- Updated dependencies [1146b76d2]
- Updated dependencies [db17f6f7b]
  - @pnpm/config@10.0.0
  - @pnpm/types@6.2.0
  - @pnpm/cli-utils@0.4.10
  - @pnpm/store-connection-manager@0.3.12
  - @pnpm/core-loggers@4.1.2
  - dependency-path@5.0.2
  - @pnpm/find-workspace-packages@2.2.8
  - @pnpm/get-context@3.0.1
  - @pnpm/lifecycle@9.1.3
  - @pnpm/link-bins@5.3.7
  - @pnpm/lockfile-utils@2.0.15
  - @pnpm/lockfile-walker@3.0.3
  - @pnpm/modules-yaml@8.0.1
  - @pnpm/normalize-registries@1.0.3
  - @pnpm/sort-packages@1.0.13
  - @pnpm/store-controller-types@8.0.2

## 2.0.13

### Patch Changes

- @pnpm/store-connection-manager@0.3.11

## 2.0.12

### Patch Changes

- @pnpm/find-workspace-packages@2.2.7
- @pnpm/link-bins@5.3.6
- @pnpm/store-connection-manager@0.3.10

## 2.0.11

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [e1ca9fc13]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/link-bins@5.3.5
  - @pnpm/config@9.2.0
  - @pnpm/get-context@3.0.0
  - @pnpm/modules-yaml@8.0.0
  - @pnpm/cli-utils@0.4.9
  - @pnpm/core-loggers@4.1.1
  - dependency-path@5.0.1
  - @pnpm/find-workspace-packages@2.2.6
  - @pnpm/lifecycle@9.1.2
  - @pnpm/lockfile-utils@2.0.14
  - @pnpm/lockfile-walker@3.0.2
  - @pnpm/normalize-registries@1.0.2
  - @pnpm/store-controller-types@8.0.1
  - @pnpm/sort-packages@1.0.12
  - @pnpm/store-connection-manager@0.3.9

## 2.0.10

### Patch Changes

- @pnpm/store-connection-manager@0.3.8

## 2.0.9

### Patch Changes

- Updated dependencies [41d92948b]
- Updated dependencies [e934b1a48]
  - dependency-path@5.0.0
  - @pnpm/cli-utils@0.4.8
  - @pnpm/lockfile-utils@2.0.13
  - @pnpm/lockfile-walker@3.0.1
  - @pnpm/link-bins@5.3.4
  - @pnpm/store-connection-manager@0.3.7
  - @pnpm/find-workspace-packages@2.2.5

## 2.0.8

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
- Updated dependencies [2ebb7af33]
- Updated dependencies [68d8dc68f]
  - @pnpm/lifecycle@9.1.1
  - @pnpm/core-loggers@4.1.0
  - @pnpm/store-connection-manager@0.3.6
  - @pnpm/get-context@2.1.2
  - @pnpm/cli-utils@0.4.7
  - @pnpm/find-workspace-packages@2.2.4

## 2.0.7

### Patch Changes

- @pnpm/store-connection-manager@0.3.5

## 2.0.6

### Patch Changes

- @pnpm/store-connection-manager@0.3.4

## 2.0.5

### Patch Changes

- @pnpm/store-connection-manager@0.3.3

## 2.0.4

### Patch Changes

- Updated dependencies [ffddf34a8]
- Updated dependencies [ffddf34a8]
- Updated dependencies [8094b2a62]
  - @pnpm/common-cli-options-help@0.2.0
  - @pnpm/config@9.1.0
  - @pnpm/lifecycle@9.1.0
  - @pnpm/cli-utils@0.4.6
  - @pnpm/find-workspace-packages@2.2.3
  - @pnpm/sort-packages@1.0.11
  - @pnpm/store-connection-manager@0.3.2

## 2.0.3

### Patch Changes

- @pnpm/store-connection-manager@0.3.1

## 2.0.2

### Patch Changes

- Updated dependencies [58c02009f]
  - @pnpm/get-context@2.1.1

## 2.0.1

### Patch Changes

- Updated dependencies [327bfbf02]
  - @pnpm/get-context@2.1.0

## 2.0.0

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.
- 3f73eaf0c: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- 802d145fc: Remove `independent-leaves` support.
- 9fbb74ecb: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [b5f66c0f2]
- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [3f73eaf0c]
- Updated dependencies [f516d266c]
- Updated dependencies [142f8caf7]
- Updated dependencies [da091c711]
- Updated dependencies [f35a3ec1c]
- Updated dependencies [42e6490d1]
- Updated dependencies [e11019b89]
- Updated dependencies [a5febb913]
- Updated dependencies [802d145fc]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [a5febb913]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [4f5801b1c]
- Updated dependencies [a5febb913]
- Updated dependencies [e3990787a]
  - @pnpm/constants@4.0.0
  - @pnpm/config@9.0.0
  - @pnpm/store-controller-types@8.0.0
  - @pnpm/get-context@2.0.0
  - @pnpm/modules-yaml@7.0.0
  - @pnpm/lockfile-walker@3.0.0
  - @pnpm/store-connection-manager@0.3.0
  - @pnpm/types@6.0.0
  - @pnpm/lifecycle@9.0.0
  - @pnpm/cli-utils@0.4.5
  - @pnpm/common-cli-options-help@0.1.6
  - @pnpm/core-loggers@4.0.2
  - dependency-path@4.0.7
  - @pnpm/find-workspace-packages@2.2.2
  - @pnpm/link-bins@5.3.3
  - @pnpm/lockfile-utils@2.0.12
  - @pnpm/normalize-registries@1.0.1
  - @pnpm/sort-packages@1.0.10

## 2.0.0-alpha.5

### Patch Changes

- a7d20d927: The peer suffix at the end of local tarball dependency paths is not encoded.
- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [16d1ac0fd]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/store-controller-types@8.0.0-alpha.4
  - @pnpm/store-connection-manager@0.3.0-alpha.5
  - @pnpm/cli-utils@0.4.5-alpha.2
  - @pnpm/find-workspace-packages@2.2.2-alpha.2
  - @pnpm/sort-packages@1.0.10-alpha.2
  - @pnpm/get-context@1.2.2-alpha.2
  - @pnpm/lockfile-utils@2.0.12-alpha.1
  - @pnpm/lockfile-walker@2.0.3-alpha.1

## 2.0.0-alpha.4

### Major Changes

- 3f73eaf0: Rename `store` to `storeDir` in `node_modules/.modules.yaml`.
- 9fbb74ec: The structure of virtual store directory changed. No subdirectory created with the registry name.
  So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
  inside `node_modules/.pnpm/<pkg>`.

### Patch Changes

- Updated dependencies [3f73eaf0]
- Updated dependencies [da091c71]
- Updated dependencies [e3990787]
  - @pnpm/get-context@2.0.0-alpha.1
  - @pnpm/modules-yaml@7.0.0-alpha.0
  - @pnpm/store-connection-manager@0.3.0-alpha.4
  - @pnpm/store-controller-types@8.0.0-alpha.3
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/lifecycle@9.0.0-alpha.1
  - @pnpm/cli-utils@0.4.5-alpha.1
  - @pnpm/config@8.3.1-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0
  - dependency-path@4.0.7-alpha.0
  - @pnpm/find-workspace-packages@2.2.2-alpha.1
  - @pnpm/link-bins@5.3.3-alpha.0
  - @pnpm/lockfile-utils@2.0.12-alpha.0
  - @pnpm/lockfile-walker@2.0.3-alpha.0
  - @pnpm/normalize-registries@1.0.1-alpha.0
  - @pnpm/sort-packages@1.0.10-alpha.1

## 2.0.0-alpha.3

### Major Changes

- b5f66c0f2: Reduce the number of directories in the virtual store directory. Don't create a subdirectory for the package version. Append the package version to the package name directory.

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/find-workspace-packages@2.2.2-alpha.0
  - @pnpm/get-context@1.2.2-alpha.0
  - @pnpm/store-connection-manager@0.2.32-alpha.3
  - @pnpm/cli-utils@0.4.5-alpha.0
  - @pnpm/sort-packages@1.0.10-alpha.0

## 1.0.11-alpha.2

### Patch Changes

- Updated dependencies [f35a3ec1c]
- Updated dependencies [42e6490d1]
  - @pnpm/lifecycle@8.2.0-alpha.0
  - @pnpm/store-controller-types@8.0.0-alpha.2
  - @pnpm/store-connection-manager@0.2.32-alpha.2

## 1.0.11-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/store-controller-types@8.0.0-alpha.1
  - @pnpm/store-connection-manager@0.2.32-alpha.1

## 1.0.11-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/store-controller-types@8.0.0-alpha.0
  - @pnpm/store-connection-manager@0.3.0-alpha.0

## 1.0.11

### Patch Changes

- Updated dependencies [2ec4c4eb9]
  - @pnpm/lifecycle@8.2.0

## 1.0.10

### Patch Changes

- 907c63a48: Dependencies updated.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/store-connection-manager@0.2.31
  - @pnpm/link-bins@5.3.2
  - @pnpm/get-context@1.2.1
  - @pnpm/lockfile-utils@2.0.11
  - @pnpm/modules-yaml@6.0.2
  - @pnpm/cli-utils@0.4.4
  - @pnpm/find-workspace-packages@2.2.1
