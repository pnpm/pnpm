# @pnpm/plugin-commands-installation

## 16.0.0

### Major Changes

- dd00eeb: Renamed dir to rootDir in the Project object.

### Patch Changes

- 84654bd: Installation with filtering should work, when `dedupe-peer-dependents` is set to `true` [#6300](https://github.com/pnpm/pnpm/issues/6300).
- Updated dependencies [1b03682]
- Updated dependencies [dd00eeb]
- Updated dependencies
- Updated dependencies [9b5b869]
  - @pnpm/config@21.6.0
  - @pnpm/filter-workspace-packages@10.0.0
  - @pnpm/plugin-commands-rebuild@12.0.0
  - @pnpm/get-context@12.0.0
  - @pnpm/resolver-base@13.0.0
  - @pnpm/workspace.find-packages@4.0.0
  - @pnpm/workspace.pkgs-graph@4.0.0
  - @pnpm/outdated@15.0.0
  - @pnpm/core@15.0.0
  - @pnpm/types@11.0.0
  - @pnpm/command@5.0.2
  - @pnpm/cli-utils@3.1.3
  - @pnpm/store-connection-manager@8.3.1
  - @pnpm/package-store@20.3.1
  - @pnpm/dedupe.check@2.0.5
  - @pnpm/pnpmfile@6.0.6
  - @pnpm/lockfile-types@7.1.2
  - @pnpm/manifest-utils@6.0.4
  - @pnpm/read-project-manifest@6.0.4
  - @pnpm/sort-packages@6.0.4

## 15.1.11

### Patch Changes

- 13e55b2: If install is performed on a subset of workspace projects, always create an up-to-date lockfile first. So, a partial install can be performed only on a fully resolved (non-partial) lockfile [#8165](https://github.com/pnpm/pnpm/issues/8165).
- 04b8363: The `getConfig` function from `@pnpm/config` now reads the `pnpm-workspace.yaml` file and stores `workspacePackagePatterns` in the `Config` object. An internal refactor was made in pnpm to reuse this value instead of re-reading `pnpm-workspace.yaml` multiple times.
- Updated dependencies [7c6c923]
- Updated dependencies [7d10394]
- Updated dependencies [d8eab39]
- Updated dependencies [13e55b2]
- Updated dependencies
- Updated dependencies [04b8363]
  - @pnpm/store-connection-manager@8.3.0
  - @pnpm/plugin-commands-rebuild@11.3.0
  - @pnpm/package-store@20.3.0
  - @pnpm/config@21.5.0
  - @pnpm/core@14.2.1
  - @pnpm/types@10.1.1
  - @pnpm/filter-workspace-packages@9.0.0
  - @pnpm/workspace.find-packages@3.0.0
  - @pnpm/cli-utils@3.1.2
  - @pnpm/dedupe.check@2.0.4
  - @pnpm/pnpmfile@6.0.5
  - @pnpm/lockfile-types@7.1.1
  - @pnpm/manifest-utils@6.0.3
  - @pnpm/read-project-manifest@6.0.3
  - @pnpm/resolver-base@12.0.2
  - @pnpm/outdated@14.0.12
  - @pnpm/workspace.pkgs-graph@3.0.6
  - @pnpm/sort-packages@6.0.3

## 15.1.10

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/plugin-commands-rebuild@11.2.0
  - @pnpm/lockfile-types@7.1.0
  - @pnpm/core@14.2.0
  - @pnpm/config@21.4.0
  - @pnpm/outdated@14.0.11
  - @pnpm/dedupe.check@2.0.3
  - @pnpm/pnpmfile@6.0.4
  - @pnpm/cli-utils@3.1.1
  - @pnpm/store-connection-manager@8.2.2
  - @pnpm/package-store@20.2.1
  - @pnpm/workspace.find-packages@2.1.1
  - @pnpm/filter-workspace-packages@8.0.10

## 15.1.9

### Patch Changes

- Updated dependencies [b7ca13f]
- Updated dependencies [b7ca13f]
  - @pnpm/cli-utils@3.1.0
  - @pnpm/workspace.find-packages@2.1.0
  - @pnpm/config@21.3.0
  - @pnpm/core@14.1.9
  - @pnpm/plugin-commands-rebuild@11.1.6
  - @pnpm/filter-workspace-packages@8.0.9
  - @pnpm/store-connection-manager@8.2.1
  - @pnpm/outdated@14.0.10
  - @pnpm/package-store@20.2.0

## 15.1.8

### Patch Changes

- @pnpm/core@14.1.8

## 15.1.7

### Patch Changes

- @pnpm/core@14.1.7

## 15.1.6

### Patch Changes

- Updated dependencies [27c33f0]
  - @pnpm/core@14.1.6

## 15.1.5

### Patch Changes

- Updated dependencies [0c08e1c]
  - @pnpm/store-connection-manager@8.2.0
  - @pnpm/package-store@20.2.0
  - @pnpm/outdated@14.0.9
  - @pnpm/workspace.pkgs-graph@3.0.5
  - @pnpm/plugin-commands-rebuild@11.1.5
  - @pnpm/pnpmfile@6.0.3
  - @pnpm/core@14.1.5
  - @pnpm/filter-workspace-packages@8.0.8
  - @pnpm/config@21.2.3
  - @pnpm/cli-utils@3.0.7
  - @pnpm/workspace.find-packages@2.0.7

## 15.1.4

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-types@7.0.0
  - @pnpm/core@14.1.4
  - @pnpm/cli-utils@3.0.6
  - @pnpm/config@21.2.2
  - @pnpm/dedupe.check@2.0.2
  - @pnpm/plugin-commands-rebuild@11.1.4
  - @pnpm/pnpmfile@6.0.2
  - @pnpm/manifest-utils@6.0.2
  - @pnpm/read-project-manifest@6.0.2
  - @pnpm/resolver-base@12.0.1
  - @pnpm/outdated@14.0.8
  - @pnpm/package-store@20.1.2
  - @pnpm/filter-workspace-packages@8.0.7
  - @pnpm/workspace.find-packages@2.0.6
  - @pnpm/workspace.pkgs-graph@3.0.4
  - @pnpm/sort-packages@6.0.2
  - @pnpm/store-connection-manager@8.1.4

## 15.1.3

### Patch Changes

- Updated dependencies [a7aef51]
- Updated dependencies [37538f5]
  - @pnpm/error@6.0.1
  - @pnpm/command@5.0.1
  - @pnpm/core@14.1.3
  - @pnpm/outdated@14.0.7
  - @pnpm/cli-utils@3.0.5
  - @pnpm/config@21.2.1
  - @pnpm/dedupe.check@2.0.1
  - @pnpm/plugin-commands-rebuild@11.1.3
  - @pnpm/pnpmfile@6.0.1
  - @pnpm/manifest-utils@6.0.1
  - @pnpm/read-project-manifest@6.0.1
  - @pnpm/store-connection-manager@8.1.3
  - @pnpm/filter-workspace-packages@8.0.6
  - @pnpm/find-workspace-dir@7.0.1
  - @pnpm/workspace.find-packages@2.0.5
  - @pnpm/package-store@20.1.1
  - @pnpm/workspace.pkgs-graph@3.0.3

## 15.1.2

### Patch Changes

- @pnpm/core@14.1.2
- @pnpm/outdated@14.0.6
- @pnpm/workspace.pkgs-graph@3.0.2
- @pnpm/cli-utils@3.0.4
- @pnpm/filter-workspace-packages@8.0.5
- @pnpm/plugin-commands-rebuild@11.1.2
- @pnpm/workspace.find-packages@2.0.4
- @pnpm/package-store@20.1.0
- @pnpm/store-connection-manager@8.1.2

## 15.1.1

### Patch Changes

- @pnpm/plugin-commands-rebuild@11.1.1
- @pnpm/core@14.1.1
- @pnpm/sort-packages@6.0.1
- @pnpm/outdated@14.0.5
- @pnpm/workspace.pkgs-graph@3.0.1
- @pnpm/filter-workspace-packages@8.0.4
- @pnpm/package-store@20.1.0
- @pnpm/store-connection-manager@8.1.1

## 15.1.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/store-connection-manager@8.1.0
  - @pnpm/plugin-commands-rebuild@11.1.0
  - @pnpm/package-store@20.1.0
  - @pnpm/core@14.1.0
  - @pnpm/config@21.2.0
  - @pnpm/outdated@14.0.4
  - @pnpm/cli-utils@3.0.3
  - @pnpm/workspace.find-packages@2.0.3
  - @pnpm/filter-workspace-packages@8.0.3

## 15.0.7

### Patch Changes

- Updated dependencies [8209342]
  - @pnpm/core@14.0.7
  - @pnpm/outdated@14.0.3
  - @pnpm/plugin-commands-rebuild@11.0.5
  - @pnpm/package-store@20.0.1
  - @pnpm/store-connection-manager@8.0.4

## 15.0.6

### Patch Changes

- Updated dependencies [6b6ca69]
  - @pnpm/core@14.0.6
  - @pnpm/outdated@14.0.2
  - @pnpm/plugin-commands-rebuild@11.0.4

## 15.0.5

### Patch Changes

- @pnpm/core@14.0.5
- @pnpm/package-store@20.0.1
- @pnpm/store-connection-manager@8.0.3
- @pnpm/plugin-commands-rebuild@11.0.3

## 15.0.4

### Patch Changes

- Updated dependencies [a80b539]
  - @pnpm/cli-utils@3.0.2
  - @pnpm/plugin-commands-rebuild@11.0.2
  - @pnpm/core@14.0.4
  - @pnpm/workspace.find-packages@2.0.2
  - @pnpm/filter-workspace-packages@8.0.2
  - @pnpm/outdated@14.0.1
  - @pnpm/package-store@20.0.0
  - @pnpm/store-connection-manager@8.0.2

## 15.0.3

### Patch Changes

- @pnpm/core@14.0.3

## 15.0.2

### Patch Changes

- Updated dependencies [461d76a]
  - @pnpm/core@14.0.2

## 15.0.1

### Patch Changes

- Updated dependencies [e0f47f4]
  - @pnpm/config@21.1.0
  - @pnpm/cli-utils@3.0.1
  - @pnpm/plugin-commands-rebuild@11.0.1
  - @pnpm/store-connection-manager@8.0.1
  - @pnpm/core@14.0.1
  - @pnpm/workspace.find-packages@2.0.1
  - @pnpm/filter-workspace-packages@8.0.1

## 15.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- d4e13ca: Add `--ignore-scripts` argument to `prune` command

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [aa33269]
- Updated dependencies [3ded840]
- Updated dependencies [cdd8365]
- Updated dependencies [c692f80]
- Updated dependencies [43cdd87]
- Updated dependencies [2d9e3b8]
- Updated dependencies [086b69c]
- Updated dependencies [9f8948c]
- Updated dependencies [d381a60]
- Updated dependencies [ca2be03]
- Updated dependencies [0fa26f4]
- Updated dependencies [3477ee5]
- Updated dependencies [cfa33f1]
- Updated dependencies [e748162]
- Updated dependencies [e5fbac3]
- Updated dependencies [2b89155]
- Updated dependencies [27a96a8]
- Updated dependencies [60839fc]
- Updated dependencies [b13d2dc]
- Updated dependencies [730929e]
- Updated dependencies [98566d9]
  - @pnpm/store-connection-manager@8.0.0
  - @pnpm/plugin-commands-rebuild@11.0.0
  - @pnpm/core@14.0.0
  - @pnpm/types@10.0.0
  - @pnpm/config@21.0.0
  - @pnpm/error@6.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/filter-workspace-packages@8.0.0
  - @pnpm/read-project-manifest@6.0.0
  - @pnpm/parse-wanted-dependency@6.0.0
  - @pnpm/find-workspace-dir@7.0.0
  - @pnpm/common-cli-options-help@2.0.0
  - @pnpm/manifest-utils@6.0.0
  - @pnpm/lockfile-types@6.0.0
  - @pnpm/resolver-base@12.0.0
  - @pnpm/workspace.find-packages@2.0.0
  - @pnpm/sort-packages@6.0.0
  - @pnpm/workspace.pkgs-graph@3.0.0
  - @pnpm/package-store@20.0.0
  - @pnpm/outdated@14.0.0
  - @pnpm/matcher@6.0.0
  - @pnpm/graceful-fs@4.0.0
  - @pnpm/pnpmfile@6.0.0
  - @pnpm/cli-utils@3.0.0
  - @pnpm/dedupe.check@2.0.0
  - @pnpm/command@5.0.0

## 14.2.0

### Minor Changes

- 31054a63e: Running `pnpm update -r --latest` will no longer downgrade prerelease dependencies [#7436](https://github.com/pnpm/pnpm/issues/7436).

### Patch Changes

- f43bdcf45: Throw an error if `pnpm update --latest` runs with arguments containing versions specs. For instance, `pnpm update --latest foo@next` is not allowed [#7567](https://github.com/pnpm/pnpm/pull/7567).
- Updated dependencies [31054a63e]
  - @pnpm/resolver-base@11.1.0
  - @pnpm/core@13.4.0
  - @pnpm/package-store@19.0.15
  - @pnpm/plugin-commands-rebuild@10.0.17
  - @pnpm/pnpmfile@5.0.20
  - @pnpm/outdated@13.0.42
  - @pnpm/workspace.pkgs-graph@2.0.14
  - @pnpm/cli-utils@2.1.9
  - @pnpm/store-connection-manager@7.0.26
  - @pnpm/config@20.4.2
  - @pnpm/filter-workspace-packages@7.2.11
  - @pnpm/workspace.find-packages@1.1.10

## 14.1.3

### Patch Changes

- @pnpm/plugin-commands-rebuild@10.0.16
- @pnpm/core@13.3.3
- @pnpm/outdated@13.0.41
- @pnpm/package-store@19.0.14
- @pnpm/store-connection-manager@7.0.25

## 14.1.2

### Patch Changes

- Updated dependencies [d9564e354]
- Updated dependencies [ff10acade]
  - @pnpm/config@20.4.1
  - @pnpm/core@13.3.2
  - @pnpm/plugin-commands-rebuild@10.0.15
  - @pnpm/outdated@13.0.40
  - @pnpm/store-connection-manager@7.0.24
  - @pnpm/cli-utils@2.1.8
  - @pnpm/package-store@19.0.14
  - @pnpm/workspace.find-packages@1.1.9
  - @pnpm/filter-workspace-packages@7.2.10

## 14.1.1

### Patch Changes

- @pnpm/package-store@19.0.14
- @pnpm/core@13.3.1
- @pnpm/store-connection-manager@7.0.23
- @pnpm/plugin-commands-rebuild@10.0.14
- @pnpm/outdated@13.0.39

## 14.1.0

### Minor Changes

- 064aeb681: The `pnpm dedupe` command now accepts more command line options that the `pnpm install` command also accepts. Example: `pnpm dedupe --store-dir=local-store-dir`

### Patch Changes

- Updated dependencies [c597f72ec]
  - @pnpm/core@13.3.0
  - @pnpm/config@20.4.0
  - @pnpm/cli-utils@2.1.7
  - @pnpm/plugin-commands-rebuild@10.0.13
  - @pnpm/store-connection-manager@7.0.22
  - @pnpm/workspace.find-packages@1.1.8
  - @pnpm/filter-workspace-packages@7.2.9

## 14.0.15

### Patch Changes

- 619e9ed6f: Better support for light themed terminals by the `pnpm update --interactive` command [#7439](https://github.com/pnpm/pnpm/issues/7439).
- Updated dependencies [4e71066dd]
- Updated dependencies [8fd4e349a]
- Updated dependencies [4d34684f1]
  - @pnpm/common-cli-options-help@1.1.0
  - @pnpm/config@20.3.0
  - @pnpm/outdated@13.0.38
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/types@9.4.2
  - @pnpm/plugin-commands-rebuild@10.0.12
  - @pnpm/cli-utils@2.1.6
  - @pnpm/store-connection-manager@7.0.21
  - @pnpm/core@13.2.1
  - @pnpm/workspace.pkgs-graph@2.0.13
  - @pnpm/package-store@19.0.13
  - @pnpm/dedupe.check@1.0.7
  - @pnpm/pnpmfile@5.0.19
  - @pnpm/manifest-utils@5.0.7
  - @pnpm/read-project-manifest@5.0.10
  - @pnpm/resolver-base@11.0.2
  - @pnpm/filter-workspace-packages@7.2.8
  - @pnpm/workspace.find-packages@1.1.7
  - @pnpm/sort-packages@5.0.9

## 14.0.14

### Patch Changes

- Updated dependencies
- Updated dependencies [672c559e4]
  - @pnpm/lockfile-types@5.1.4
  - @pnpm/types@9.4.1
  - @pnpm/core@13.2.0
  - @pnpm/config@20.2.0
  - @pnpm/dedupe.check@1.0.6
  - @pnpm/plugin-commands-rebuild@10.0.11
  - @pnpm/pnpmfile@5.0.18
  - @pnpm/cli-utils@2.1.5
  - @pnpm/manifest-utils@5.0.6
  - @pnpm/read-project-manifest@5.0.9
  - @pnpm/resolver-base@11.0.1
  - @pnpm/outdated@13.0.37
  - @pnpm/package-store@19.0.12
  - @pnpm/filter-workspace-packages@7.2.7
  - @pnpm/workspace.find-packages@1.1.6
  - @pnpm/sort-packages@5.0.8
  - @pnpm/store-connection-manager@7.0.20
  - @pnpm/workspace.pkgs-graph@2.0.12

## 14.0.13

### Patch Changes

- @pnpm/core@13.1.12
- @pnpm/plugin-commands-rebuild@10.0.10
- @pnpm/outdated@13.0.36
- @pnpm/package-store@19.0.11
- @pnpm/store-connection-manager@7.0.19

## 14.0.12

### Patch Changes

- @pnpm/core@13.1.11

## 14.0.11

### Patch Changes

- 6558d1865: When `dedupe-direct-deps` is set to `true`, commands of dependencies should be deduplicated [#7359](https://github.com/pnpm/pnpm/pull/7359).
- Updated dependencies [6558d1865]
  - @pnpm/core@13.1.10
  - @pnpm/cli-utils@2.1.4
  - @pnpm/workspace.find-packages@1.1.5
  - @pnpm/outdated@13.0.35
  - @pnpm/package-store@19.0.10
  - @pnpm/store-connection-manager@7.0.18
  - @pnpm/plugin-commands-rebuild@10.0.9
  - @pnpm/filter-workspace-packages@7.2.6

## 14.0.10

### Patch Changes

- @pnpm/cli-utils@2.1.3
- @pnpm/plugin-commands-rebuild@10.0.8
- @pnpm/workspace.find-packages@1.1.4
- @pnpm/core@13.1.9
- @pnpm/outdated@13.0.34
- @pnpm/package-store@19.0.10
- @pnpm/store-connection-manager@7.0.17
- @pnpm/filter-workspace-packages@7.2.5

## 14.0.9

### Patch Changes

- @pnpm/plugin-commands-rebuild@10.0.7
- @pnpm/core@13.1.9
- @pnpm/outdated@13.0.33
- @pnpm/workspace.pkgs-graph@2.0.11
- @pnpm/filter-workspace-packages@7.2.4
- @pnpm/package-store@19.0.10
- @pnpm/store-connection-manager@7.0.16

## 14.0.8

### Patch Changes

- @pnpm/core@13.1.8
- @pnpm/package-store@19.0.10
- @pnpm/plugin-commands-rebuild@10.0.6
- @pnpm/workspace.find-packages@1.1.3
- @pnpm/store-connection-manager@7.0.15
- @pnpm/filter-workspace-packages@7.2.3
- @pnpm/outdated@13.0.32

## 14.0.7

### Patch Changes

- @pnpm/plugin-commands-rebuild@10.0.5
- @pnpm/core@13.1.7
- @pnpm/outdated@13.0.31

## 14.0.6

### Patch Changes

- Updated dependencies [291607c5a]
  - @pnpm/package-store@19.0.9
  - @pnpm/workspace.find-packages@1.1.2
  - @pnpm/plugin-commands-rebuild@10.0.4
  - @pnpm/pnpmfile@5.0.17
  - @pnpm/core@13.1.6
  - @pnpm/store-connection-manager@7.0.14
  - @pnpm/filter-workspace-packages@7.2.2
  - @pnpm/outdated@13.0.30
  - @pnpm/config@20.1.2
  - @pnpm/cli-utils@2.1.2

## 14.0.5

### Patch Changes

- @pnpm/core@13.1.5

## 14.0.4

### Patch Changes

- @pnpm/plugin-commands-rebuild@10.0.3
- @pnpm/core@13.1.4
- @pnpm/outdated@13.0.29
- @pnpm/package-store@19.0.8
- @pnpm/store-connection-manager@7.0.13

## 14.0.3

### Patch Changes

- Updated dependencies [4c2450208]
- Updated dependencies [cfc017ee3]
- Updated dependencies [7ea45afbe]
  - @pnpm/resolver-base@11.0.0
  - @pnpm/core@13.1.3
  - @pnpm/plugin-commands-rebuild@10.0.2
  - @pnpm/outdated@13.0.28
  - @pnpm/package-store@19.0.8
  - @pnpm/workspace.pkgs-graph@2.0.10
  - @pnpm/store-connection-manager@7.0.12
  - @pnpm/pnpmfile@5.0.16
  - @pnpm/filter-workspace-packages@7.2.1
  - @pnpm/config@20.1.1
  - @pnpm/cli-utils@2.1.1
  - @pnpm/workspace.find-packages@1.1.1

## 14.0.2

### Patch Changes

- @pnpm/plugin-commands-rebuild@10.0.1
- @pnpm/core@13.1.2
- @pnpm/outdated@13.0.27
- @pnpm/package-store@19.0.7
- @pnpm/store-connection-manager@7.0.11

## 14.0.1

### Patch Changes

- @pnpm/core@13.1.1

## 14.0.0

### Major Changes

- d6592964f: `rootProjectManifestDir` is a required field.

### Patch Changes

- d6592964f: `pnpm fetch` read settings from root project manifest.
- Updated dependencies [43ce9e4a6]
- Updated dependencies [6390033cd]
- Updated dependencies [d6592964f]
  - @pnpm/filter-workspace-packages@7.2.0
  - @pnpm/plugin-commands-rebuild@10.0.0
  - @pnpm/workspace.find-packages@1.1.0
  - @pnpm/core@13.1.0
  - @pnpm/types@9.4.0
  - @pnpm/cli-utils@2.1.0
  - @pnpm/config@20.1.0
  - @pnpm/package-store@19.0.7
  - @pnpm/pnpmfile@5.0.15
  - @pnpm/dedupe.check@1.0.5
  - @pnpm/lockfile-types@5.1.3
  - @pnpm/manifest-utils@5.0.5
  - @pnpm/read-project-manifest@5.0.8
  - @pnpm/resolver-base@10.0.4
  - @pnpm/outdated@13.0.26
  - @pnpm/sort-packages@5.0.7
  - @pnpm/store-connection-manager@7.0.10
  - @pnpm/workspace.pkgs-graph@2.0.9

## 13.2.6

### Patch Changes

- @pnpm/plugin-commands-rebuild@9.2.6
- @pnpm/core@13.0.2
- @pnpm/outdated@13.0.25
- @pnpm/package-store@19.0.6
- @pnpm/store-connection-manager@7.0.9

## 13.2.5

### Patch Changes

- Updated dependencies [4246f41be]
  - @pnpm/plugin-commands-rebuild@9.2.5
  - @pnpm/sort-packages@5.0.6
  - @pnpm/core@13.0.1
  - @pnpm/package-store@19.0.6
  - @pnpm/store-connection-manager@7.0.8
  - @pnpm/outdated@13.0.24

## 13.2.4

### Patch Changes

- bc83798d4: Optimize selection result output of `pnpm update --interactive` [7109](https://github.com/pnpm/pnpm/issues/7109)
- 46dc34dcc: When `shared-workspace-lockfile` is set to `false`, read the pnpm settings from `package.json` files that are nested. This was broken in pnpm v8.9.0 [#7184](https://github.com/pnpm/pnpm/issues/7184).
- Updated dependencies [01bc58e2c]
- Updated dependencies [ac5abd3ff]
- Updated dependencies [b60bb6cbe]
  - @pnpm/package-store@19.0.5
  - @pnpm/core@13.0.0
  - @pnpm/config@20.0.0
  - @pnpm/outdated@13.0.23
  - @pnpm/workspace.pkgs-graph@2.0.8
  - @pnpm/store-connection-manager@7.0.7
  - @pnpm/plugin-commands-rebuild@9.2.4
  - @pnpm/cli-utils@2.0.24
  - @pnpm/filter-workspace-packages@7.1.4
  - @pnpm/workspace.find-packages@1.0.14

## 13.2.3

### Patch Changes

- Updated dependencies [4aa41ef3a]
  - @pnpm/plugin-commands-rebuild@9.2.3
  - @pnpm/package-store@19.0.4
  - @pnpm/core@12.2.2
  - @pnpm/store-connection-manager@7.0.6
  - @pnpm/outdated@13.0.22

## 13.2.2

### Patch Changes

- @pnpm/package-store@19.0.3
- @pnpm/core@12.2.1
- @pnpm/store-connection-manager@7.0.5
- @pnpm/plugin-commands-rebuild@9.2.2
- @pnpm/outdated@13.0.21

## 13.2.1

### Patch Changes

- Updated dependencies [b1dd0ee58]
  - @pnpm/config@19.2.1
  - @pnpm/cli-utils@2.0.23
  - @pnpm/plugin-commands-rebuild@9.2.1
  - @pnpm/store-connection-manager@7.0.4
  - @pnpm/workspace.find-packages@1.0.13
  - @pnpm/filter-workspace-packages@7.1.3

## 13.2.0

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

- 832e28826: Add `disallow-workspace-cycles` option to error instead of warn about cyclic dependencies

### Patch Changes

- 12f45a83d: Use `neverBuiltDependencies` and `onlyBuiltDependencies` from the root `package.json` of the workspace, when `shared-workspace-lockfile` is set to `false` [#7141](https://github.com/pnpm/pnpm/pull/7141).
- Updated dependencies [d774a3196]
- Updated dependencies [12f45a83d]
- Updated dependencies [d774a3196]
- Updated dependencies [832e28826]
  - @pnpm/config@19.2.0
  - @pnpm/plugin-commands-rebuild@9.2.0
  - @pnpm/core@12.2.0
  - @pnpm/types@9.3.0
  - @pnpm/cli-utils@2.0.22
  - @pnpm/store-connection-manager@7.0.3
  - @pnpm/dedupe.check@1.0.4
  - @pnpm/pnpmfile@5.0.14
  - @pnpm/lockfile-types@5.1.2
  - @pnpm/manifest-utils@5.0.4
  - @pnpm/read-project-manifest@5.0.7
  - @pnpm/resolver-base@10.0.3
  - @pnpm/outdated@13.0.20
  - @pnpm/package-store@19.0.2
  - @pnpm/workspace.find-packages@1.0.12
  - @pnpm/sort-packages@5.0.5
  - @pnpm/workspace.pkgs-graph@2.0.7
  - @pnpm/filter-workspace-packages@7.1.2

## 13.1.8

### Patch Changes

- @pnpm/core@12.1.3

## 13.1.7

### Patch Changes

- ba48fe0bc: Pass through the `--ignore-scripts` command to install, when running `pnpm dedupe --ignore-scripts` [#7102](https://github.com/pnpm/pnpm/issues/7102).
- Updated dependencies [1f32d3eb8]
- Updated dependencies [ee328fd25]
- Updated dependencies [f394cfccd]
  - @pnpm/core@12.1.2
  - @pnpm/config@19.1.0
  - @pnpm/cli-utils@2.0.21
  - @pnpm/plugin-commands-rebuild@9.1.16
  - @pnpm/store-connection-manager@7.0.2
  - @pnpm/outdated@13.0.19
  - @pnpm/workspace.find-packages@1.0.11
  - @pnpm/package-store@19.0.1
  - @pnpm/filter-workspace-packages@7.1.1

## 13.1.6

### Patch Changes

- Updated dependencies [a6f5e5c9c]
  - @pnpm/filter-workspace-packages@7.1.0
  - @pnpm/plugin-commands-rebuild@9.1.15
  - @pnpm/cli-utils@2.0.20
  - @pnpm/core@12.1.1
  - @pnpm/workspace.find-packages@1.0.10
  - @pnpm/outdated@13.0.18
  - @pnpm/package-store@19.0.0
  - @pnpm/store-connection-manager@7.0.1

## 13.1.5

### Patch Changes

- Updated dependencies [9caa33d53]
- Updated dependencies [9caa33d53]
  - @pnpm/store-connection-manager@7.0.0
  - @pnpm/core@13.0.0
  - @pnpm/graceful-fs@3.2.0
  - @pnpm/plugin-commands-rebuild@9.1.14
  - @pnpm/pnpmfile@5.0.13
  - @pnpm/package-store@19.0.0
  - @pnpm/read-project-manifest@5.0.6
  - @pnpm/config@19.0.3
  - @pnpm/outdated@13.0.17
  - @pnpm/cli-utils@2.0.19
  - @pnpm/workspace.pkgs-graph@2.0.6
  - @pnpm/workspace.find-packages@1.0.9
  - @pnpm/filter-workspace-packages@7.0.19

## 13.1.4

### Patch Changes

- @pnpm/package-store@18.0.1
- @pnpm/core@12.0.1
- @pnpm/store-connection-manager@6.2.1
- @pnpm/plugin-commands-rebuild@9.1.13
- @pnpm/outdated@13.0.16

## 13.1.3

### Patch Changes

- Updated dependencies [03cdccc6e]
- Updated dependencies [48dcd108c]
  - @pnpm/store-connection-manager@6.2.0
  - @pnpm/core@12.0.0
  - @pnpm/plugin-commands-rebuild@9.1.12
  - @pnpm/pnpmfile@5.0.12
  - @pnpm/package-store@18.0.0
  - @pnpm/config@19.0.2
  - @pnpm/cli-utils@2.0.18
  - @pnpm/outdated@13.0.15
  - @pnpm/workspace.find-packages@1.0.8
  - @pnpm/filter-workspace-packages@7.0.18

## 13.1.2

### Patch Changes

- @pnpm/plugin-commands-rebuild@9.1.11
- @pnpm/core@11.0.2
- @pnpm/package-store@17.0.2
- @pnpm/store-connection-manager@6.1.3
- @pnpm/config@19.0.1
- @pnpm/outdated@13.0.14

## 13.1.1

### Patch Changes

- @pnpm/store-connection-manager@6.1.2
- @pnpm/plugin-commands-rebuild@9.1.10
- @pnpm/core@11.0.1
- @pnpm/package-store@17.0.1
- @pnpm/pnpmfile@5.0.11
- @pnpm/outdated@13.0.13
- @pnpm/config@19.0.1
- @pnpm/cli-utils@2.0.17
- @pnpm/workspace.find-packages@1.0.7
- @pnpm/filter-workspace-packages@7.0.17

## 13.1.0

### Minor Changes

- 81e5ada3a: Improve the `pnpm update --interactive` output. Dependencies are grouped by dependency types and a new column is added with links to the outdated package's docs [#6978](https://github.com/pnpm/pnpm/pull/6978).

### Patch Changes

- ef3609049: Set `skipIfHasSideEffectsCache` to `true` when calling rebuild, fixing side effect caching issue when lockfile isn't shared [#6890](https://github.com/pnpm/pnpm/issues/6890).
- e0474bc4c: Warn when linking a package with peerDependencies [615](https://github.com/pnpm/pnpm/issues/615).
- bf21c9bf3: Add support for npm lockfile v3 in `pnpm import` [#6233](https://github.com/pnpm/pnpm/issues/6233).
- Updated dependencies [41c2b65cf]
- Updated dependencies [cb8bcc8df]
- Updated dependencies [494f87544]
- Updated dependencies [083bbf590]
- Updated dependencies [e9aa6f682]
  - @pnpm/workspace.pkgs-graph@2.0.5
  - @pnpm/config@19.0.0
  - @pnpm/package-store@17.0.0
  - @pnpm/graceful-fs@3.1.0
  - @pnpm/plugin-commands-rebuild@9.1.9
  - @pnpm/core@11.0.0
  - @pnpm/outdated@13.0.12
  - @pnpm/filter-workspace-packages@7.0.16
  - @pnpm/cli-utils@2.0.16
  - @pnpm/store-connection-manager@6.1.1
  - @pnpm/pnpmfile@5.0.10
  - @pnpm/read-project-manifest@5.0.5
  - @pnpm/workspace.find-packages@1.0.6

## 13.0.25

### Patch Changes

- 12b0f0976: `pnpm patch` should works correctly when shared-workspace-file is false [#6885](https://github.com/pnpm/pnpm/issues/6885)
- Updated dependencies [12b0f0976]
- Updated dependencies [ecad8a724]
- Updated dependencies [92f42224c]
  - @pnpm/filter-workspace-packages@7.0.15
  - @pnpm/workspace.find-packages@1.0.5
  - @pnpm/core@10.2.15
  - @pnpm/store-connection-manager@6.1.0
  - @pnpm/package-store@16.1.0
  - @pnpm/plugin-commands-rebuild@9.1.8
  - @pnpm/cli-utils@2.0.15
  - @pnpm/outdated@13.0.11

## 13.0.24

### Patch Changes

- 78d43a862: Always set `dedupe-peer-dependents` to `false`, when running installation during deploy [#6858](https://github.com/pnpm/pnpm/issues/6858).
- Updated dependencies [5e7ee2473]
  - @pnpm/core@10.2.14
  - @pnpm/plugin-commands-rebuild@9.1.7
  - @pnpm/outdated@13.0.10
  - @pnpm/package-store@16.0.12
  - @pnpm/store-connection-manager@6.0.24

## 13.0.23

### Patch Changes

- @pnpm/cli-utils@2.0.14
- @pnpm/core@10.2.13
- @pnpm/package-store@16.0.12
- @pnpm/plugin-commands-rebuild@9.1.6
- @pnpm/workspace.find-packages@1.0.4
- @pnpm/store-connection-manager@6.0.23
- @pnpm/filter-workspace-packages@7.0.14

## 13.0.22

### Patch Changes

- @pnpm/core@10.2.12
- @pnpm/package-store@16.0.11
- @pnpm/plugin-commands-rebuild@9.1.5
- @pnpm/store-connection-manager@6.0.22
- @pnpm/config@18.4.4

## 13.0.21

### Patch Changes

- Updated dependencies [388a13b56]
  - @pnpm/core@10.2.11
  - @pnpm/plugin-commands-rebuild@9.1.4
  - @pnpm/package-store@16.0.10
  - @pnpm/store-connection-manager@6.0.21
  - @pnpm/outdated@13.0.9
  - @pnpm/config@18.4.4

## 13.0.20

### Patch Changes

- @pnpm/plugin-commands-rebuild@9.1.3
- @pnpm/core@10.2.10
- @pnpm/package-store@16.0.9
- @pnpm/store-connection-manager@6.0.20
- @pnpm/config@18.4.4

## 13.0.19

### Patch Changes

- @pnpm/core@10.2.9
- @pnpm/outdated@13.0.8
- @pnpm/package-store@16.0.8
- @pnpm/store-connection-manager@6.0.19
- @pnpm/plugin-commands-rebuild@9.1.2

## 13.0.18

### Patch Changes

- Updated dependencies [aa2ae8fe2]
- Updated dependencies [b8cb91cf4]
  - @pnpm/types@9.2.0
  - @pnpm/core@10.2.9
  - @pnpm/cli-utils@2.0.13
  - @pnpm/config@18.4.4
  - @pnpm/dedupe.check@1.0.3
  - @pnpm/plugin-commands-rebuild@9.1.1
  - @pnpm/pnpmfile@5.0.9
  - @pnpm/lockfile-types@5.1.1
  - @pnpm/manifest-utils@5.0.3
  - @pnpm/read-project-manifest@5.0.4
  - @pnpm/resolver-base@10.0.2
  - @pnpm/outdated@13.0.7
  - @pnpm/package-store@16.0.8
  - @pnpm/workspace.find-packages@1.0.3
  - @pnpm/sort-packages@5.0.4
  - @pnpm/store-connection-manager@6.0.18
  - @pnpm/filter-workspace-packages@7.0.13

## 13.0.17

### Patch Changes

- Updated dependencies [16bbac8d5]
- Updated dependencies [002f6febe]
  - @pnpm/plugin-commands-rebuild@9.1.0
  - @pnpm/core@10.2.8

## 13.0.16

### Patch Changes

- Updated dependencies [b4892acc5]
  - @pnpm/read-project-manifest@5.0.3
  - @pnpm/cli-utils@2.0.12
  - @pnpm/config@18.4.3
  - @pnpm/core@10.2.7
  - @pnpm/plugin-commands-rebuild@9.0.7
  - @pnpm/workspace.find-packages@1.0.2
  - @pnpm/store-connection-manager@6.0.17
  - @pnpm/filter-workspace-packages@7.0.12
  - @pnpm/outdated@13.0.6
  - @pnpm/package-store@16.0.7

## 13.0.15

### Patch Changes

- Updated dependencies [b81cefdcd]
- Updated dependencies [dddb8ad71]
  - @pnpm/core@10.2.6
  - @pnpm/plugin-commands-rebuild@9.0.6
  - @pnpm/package-store@16.0.7
  - @pnpm/store-connection-manager@6.0.16
  - @pnpm/config@18.4.2
  - @pnpm/outdated@13.0.5

## 13.0.14

### Patch Changes

- @pnpm/core@10.2.5
- @pnpm/outdated@13.0.4
- @pnpm/package-store@16.0.6
- @pnpm/store-connection-manager@6.0.15
- @pnpm/plugin-commands-rebuild@9.0.5

## 13.0.13

### Patch Changes

- @pnpm/core@10.2.5
- @pnpm/outdated@13.0.3
- @pnpm/package-store@16.0.6
- @pnpm/store-connection-manager@6.0.14
- @pnpm/plugin-commands-rebuild@9.0.4

## 13.0.12

### Patch Changes

- @pnpm/core@10.2.4

## 13.0.11

### Patch Changes

- 0b830f947: `pnpm update --global --latest` should work [#3779](https://github.com/pnpm/pnpm/issues/3779).
- Updated dependencies [e2d631217]
  - @pnpm/config@18.4.2
  - @pnpm/cli-utils@2.0.11
  - @pnpm/plugin-commands-rebuild@9.0.3
  - @pnpm/store-connection-manager@6.0.13
  - @pnpm/core@10.2.3
  - @pnpm/package-store@16.0.6
  - @pnpm/workspace.find-packages@1.0.1
  - @pnpm/filter-workspace-packages@7.0.11

## 13.0.10

### Patch Changes

- Updated dependencies [bd235794d]
- Updated dependencies [d9da627cd]
- Updated dependencies [302ebffc5]
- Updated dependencies [bd235794d]
  - @pnpm/workspace.find-packages@1.0.0
  - @pnpm/core@10.2.2
  - @pnpm/constants@7.1.1
  - @pnpm/plugin-commands-rebuild@9.0.2
  - @pnpm/filter-workspace-packages@7.0.10
  - @pnpm/outdated@13.0.2
  - @pnpm/config@18.4.1
  - @pnpm/error@5.0.2
  - @pnpm/cli-utils@2.0.10
  - @pnpm/store-connection-manager@6.0.12
  - @pnpm/dedupe.check@1.0.2
  - @pnpm/pnpmfile@5.0.8
  - @pnpm/manifest-utils@5.0.2
  - @pnpm/read-project-manifest@5.0.2
  - @pnpm/find-workspace-dir@6.0.2
  - @pnpm/package-store@16.0.5

## 13.0.9

### Patch Changes

- d55b41a8b: Dependencies have been updated.
- Updated dependencies [4b97f1f07]
- Updated dependencies [d55b41a8b]
  - @pnpm/package-store@16.0.4
  - @pnpm/core@10.2.1
  - @pnpm/plugin-commands-rebuild@9.0.1
  - @pnpm/store-connection-manager@6.0.11
  - @pnpm/outdated@13.0.1
  - @pnpm/config@18.4.0

## 13.0.8

### Patch Changes

- 04a279881: Should respect ignore patterns in updateConfig.ignoreDependencies [#6548](https://github.com/pnpm/pnpm/issues/6548)
- Updated dependencies [a53ef4d19]
- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
- Updated dependencies [301b8e2da]
- Updated dependencies [04a279881]
- Updated dependencies [9c4ae87bd]
  - @pnpm/core@10.2.0
  - @pnpm/lockfile-types@5.1.0
  - @pnpm/types@9.1.0
  - @pnpm/manifest-utils@5.0.1
  - @pnpm/constants@7.1.0
  - @pnpm/config@18.4.0
  - @pnpm/outdated@13.0.0
  - @pnpm/plugin-commands-rebuild@9.0.0
  - @pnpm/dedupe.check@1.0.1
  - @pnpm/pnpmfile@5.0.7
  - @pnpm/cli-utils@2.0.9
  - @pnpm/read-project-manifest@5.0.1
  - @pnpm/resolver-base@10.0.1
  - @pnpm/package-store@16.0.3
  - @pnpm/find-workspace-packages@6.0.9
  - @pnpm/sort-packages@5.0.3
  - @pnpm/error@5.0.1
  - @pnpm/store-connection-manager@6.0.10
  - @pnpm/filter-workspace-packages@7.0.9
  - @pnpm/find-workspace-dir@6.0.1

## 13.0.7

### Patch Changes

- Updated dependencies [ea28b6f6c]
  - @pnpm/plugin-commands-rebuild@8.0.11
  - @pnpm/core@10.1.2

## 13.0.6

### Patch Changes

- Updated dependencies [ee429b300]
- Updated dependencies [1de07a4af]
  - @pnpm/cli-utils@2.0.8
  - @pnpm/config@18.3.2
  - @pnpm/plugin-commands-rebuild@8.0.10
  - @pnpm/core@10.1.1
  - @pnpm/outdated@12.0.7
  - @pnpm/find-workspace-packages@6.0.8
  - @pnpm/store-connection-manager@6.0.9
  - @pnpm/filter-workspace-packages@7.0.8

## 13.0.5

### Patch Changes

- d5c40b556: `pnpm link -g <pkg-name>` should not modify the `package.json` file [#4341](https://github.com/pnpm/pnpm/issues/4341).

## 13.0.4

### Patch Changes

- Updated dependencies [1ffedcb8d]
- Updated dependencies [3fa14d7e4]
  - @pnpm/core@10.1.0
  - @pnpm/plugin-commands-rebuild@8.0.9
  - @pnpm/sort-packages@5.0.2

## 13.0.3

### Patch Changes

- Updated dependencies [42902ef85]
- Updated dependencies [2809e89ab]
  - @pnpm/core@10.0.0
  - @pnpm/config@18.3.1
  - @pnpm/outdated@12.0.6
  - @pnpm/plugin-commands-rebuild@8.0.8
  - @pnpm/cli-utils@2.0.7
  - @pnpm/store-connection-manager@6.0.8
  - @pnpm/find-workspace-packages@6.0.7
  - @pnpm/package-store@16.0.2
  - @pnpm/filter-workspace-packages@7.0.7

## 13.0.2

### Patch Changes

- @pnpm/core@9.3.1
- @pnpm/outdated@12.0.5
- @pnpm/package-store@16.0.2
- @pnpm/store-connection-manager@6.0.7
- @pnpm/plugin-commands-rebuild@8.0.7

## 13.0.1

### Patch Changes

- @pnpm/core@9.3.1

## 13.0.0

### Major Changes

- 8e7a86dd9: Remove the `dedupe` option from `InstallCommandOptions`. This was not intentionally part of the public install command's API when it was added.

### Minor Changes

- 6850bb135: Add `--check` flag to `pnpm dedupe`. No changes will be made to `node_modules` or the lockfile. Exits with a non-zero status code if changes are possible.
- 71a3ee77b: `pnpm install --resolution-only` re-runs resolution to print out any peer dependency issues [#6411](https://github.com/pnpm/pnpm/pull/6411).

### Patch Changes

- 8e7a86dd9: Internal refactor to call installDeps directly in the pnpm dedupe command handler. No behavior changes are expected with this refactor.
- 6706a7d17: Add lockfileCheck option for lockfile only diff installs
- Updated dependencies [6706a7d17]
- Updated dependencies [32f8e08c6]
- Updated dependencies [6850bb135]
- Updated dependencies [d43ccc44d]
- Updated dependencies [71a3ee77b]
- Updated dependencies [c0760128d]
  - @pnpm/core@9.3.0
  - @pnpm/config@18.3.0
  - @pnpm/dedupe.check@1.0.0
  - @pnpm/plugin-commands-rebuild@8.0.6
  - @pnpm/sort-packages@5.0.1
  - @pnpm/outdated@12.0.4
  - @pnpm/cli-utils@2.0.6
  - @pnpm/store-connection-manager@6.0.6
  - @pnpm/pnpmfile@5.0.6
  - @pnpm/workspace.pkgs-graph@2.0.4
  - @pnpm/package-store@16.0.2
  - @pnpm/find-workspace-packages@6.0.6
  - @pnpm/filter-workspace-packages@7.0.6

## 12.1.2

### Patch Changes

- e440d784f: Update yarn dependencies.
- Updated dependencies [fc8780ca9]
- Updated dependencies [080fee0b8]
- Updated dependencies [72ba638e3]
  - @pnpm/config@18.2.0
  - @pnpm/core@9.2.0
  - @pnpm/plugin-commands-rebuild@8.0.5
  - @pnpm/outdated@12.0.3
  - @pnpm/cli-utils@2.0.5
  - @pnpm/store-connection-manager@6.0.5
  - @pnpm/pnpmfile@5.0.5
  - @pnpm/find-workspace-packages@6.0.5
  - @pnpm/filter-workspace-packages@7.0.5

## 12.1.1

### Patch Changes

- Updated dependencies [c36c87c1c]
  - @pnpm/core@9.1.1
  - @pnpm/pnpmfile@5.0.4
  - @pnpm/plugin-commands-rebuild@8.0.4
  - @pnpm/outdated@12.0.2
  - @pnpm/cli-utils@2.0.4
  - @pnpm/config@18.1.1
  - @pnpm/package-store@16.0.1
  - @pnpm/find-workspace-packages@6.0.4
  - @pnpm/store-connection-manager@6.0.4
  - @pnpm/filter-workspace-packages@7.0.4

## 12.1.0

### Minor Changes

- e2cb4b63d: Add `ignore-workspace-cycles` to silence workspace cycle warning [#6308](https://github.com/pnpm/pnpm/pull/6308).

### Patch Changes

- Updated dependencies [e2cb4b63d]
- Updated dependencies [e87754df1]
- Updated dependencies [9fd0e375e]
- Updated dependencies [cd6ce11f0]
  - @pnpm/core@9.1.0
  - @pnpm/config@18.1.0
  - @pnpm/workspace.pkgs-graph@2.0.3
  - @pnpm/pnpmfile@5.0.3
  - @pnpm/cli-utils@2.0.3
  - @pnpm/plugin-commands-rebuild@8.0.3
  - @pnpm/store-connection-manager@6.0.3
  - @pnpm/outdated@12.0.1
  - @pnpm/filter-workspace-packages@7.0.3
  - @pnpm/find-workspace-packages@6.0.3
  - @pnpm/package-store@16.0.0

## 12.0.2

### Patch Changes

- Updated dependencies [35d98c7a8]
- Updated dependencies [3f0ea1def]
  - @pnpm/workspace.pkgs-graph@2.0.2
  - @pnpm/core@9.0.2
  - @pnpm/filter-workspace-packages@7.0.2
  - @pnpm/pnpmfile@5.0.2
  - @pnpm/plugin-commands-rebuild@8.0.2
  - @pnpm/config@18.0.2
  - @pnpm/cli-utils@2.0.2
  - @pnpm/store-connection-manager@6.0.2
  - @pnpm/find-workspace-packages@6.0.2

## 12.0.1

### Patch Changes

- 51445f955: Don't remove automatically installed peer dependencies from the root workspace project, when `dedupe-peer-dependents` is `true` [#6154](https://github.com/pnpm/pnpm/issues/6154).
- Updated dependencies [572068180]
  - @pnpm/workspace.pkgs-graph@2.0.1
  - @pnpm/core@9.0.1
  - @pnpm/filter-workspace-packages@7.0.1
  - @pnpm/pnpmfile@5.0.1
  - @pnpm/plugin-commands-rebuild@8.0.1
  - @pnpm/config@18.0.1
  - @pnpm/cli-utils@2.0.1
  - @pnpm/store-connection-manager@6.0.1
  - @pnpm/find-workspace-packages@6.0.1

## 12.0.0

### Major Changes

- cae85dbb1: Throw a "not implemented" error on the "pnpm ci" command.
- 7a0ce1df0: When there's a `files` field in the `package.json`, only deploy those files that are listed in it.
  Use the same logic also when injecting packages. This behavior can be changed by setting the `deploy-all-files` setting to `true` [#5911](https://github.com/pnpm/pnpm/issues/5911).
- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- 22ccf155e: Fix `Segmentation fault` error in the bundled version of pnpm [#6241](https://github.com/pnpm/pnpm/issues/6241).
- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [54591c686]
- Updated dependencies [7a0ce1df0]
- Updated dependencies [158d8cf22]
- Updated dependencies [2a2032810]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [8e35c21d1]
- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [113f0ae26]
- Updated dependencies [47e45d717]
  - @pnpm/core@9.0.0
  - @pnpm/config@18.0.0
  - @pnpm/store-connection-manager@6.0.0
  - @pnpm/filter-workspace-packages@7.0.0
  - @pnpm/read-project-manifest@5.0.0
  - @pnpm/find-workspace-packages@6.0.0
  - @pnpm/parse-wanted-dependency@5.0.0
  - @pnpm/plugin-commands-rebuild@8.0.0
  - @pnpm/find-workspace-dir@6.0.0
  - @pnpm/common-cli-options-help@1.0.0
  - @pnpm/manifest-utils@5.0.0
  - @pnpm/resolver-base@10.0.0
  - @pnpm/sort-packages@5.0.0
  - @pnpm/workspace.pkgs-graph@2.0.0
  - @pnpm/package-store@16.0.0
  - @pnpm/constants@7.0.0
  - @pnpm/outdated@12.0.0
  - @pnpm/matcher@5.0.0
  - @pnpm/graceful-fs@3.0.0
  - @pnpm/pnpmfile@5.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0
  - @pnpm/cli-utils@2.0.0
  - @pnpm/command@4.0.0

## 11.5.7

### Patch Changes

- @pnpm/plugin-commands-rebuild@7.1.7
- @pnpm/core@8.0.4
- @pnpm/pnpmfile@4.0.40
- @pnpm/config@17.0.2
- @pnpm/cli-utils@1.1.7
- @pnpm/store-connection-manager@5.2.20
- @pnpm/find-workspace-packages@5.0.42
- @pnpm/filter-workspace-packages@6.0.42

## 11.5.6

### Patch Changes

- Updated dependencies [b38d711f3]
  - @pnpm/config@17.0.1
  - @pnpm/plugin-commands-rebuild@7.1.6
  - @pnpm/core@8.0.3
  - @pnpm/cli-utils@1.1.6
  - @pnpm/store-connection-manager@5.2.19
  - @pnpm/pnpmfile@4.0.39
  - @pnpm/find-workspace-packages@5.0.41
  - @pnpm/filter-workspace-packages@6.0.41

## 11.5.5

### Patch Changes

- Updated dependencies [955874422]
- Updated dependencies [e505b58e3]
  - @pnpm/graceful-fs@2.1.0
  - @pnpm/config@17.0.0
  - @pnpm/read-project-manifest@4.1.4
  - @pnpm/core@8.0.2
  - @pnpm/outdated@11.0.21
  - @pnpm/cli-utils@1.1.5
  - @pnpm/plugin-commands-rebuild@7.1.5
  - @pnpm/store-connection-manager@5.2.18
  - @pnpm/package-store@15.1.8
  - @pnpm/pnpmfile@4.0.38
  - @pnpm/find-workspace-packages@5.0.40
  - @pnpm/filter-workspace-packages@6.0.40

## 11.5.4

### Patch Changes

- @pnpm/core@8.0.1
- @pnpm/pnpmfile@4.0.37
- @pnpm/config@16.7.2
- @pnpm/cli-utils@1.1.4
- @pnpm/plugin-commands-rebuild@7.1.4
- @pnpm/store-connection-manager@5.2.17
- @pnpm/find-workspace-packages@5.0.39
- @pnpm/filter-workspace-packages@6.0.39

## 11.5.3

### Patch Changes

- 690bead26: `dedupe-peer-dependents` should work when running an install command on a subset of workspace projects [#6171](https://github.com/pnpm/pnpm/issues/6171).
- Updated dependencies [670bea844]
  - @pnpm/core@8.0.0
  - @pnpm/pnpmfile@4.0.36
  - @pnpm/config@16.7.1
  - @pnpm/cli-utils@1.1.3
  - @pnpm/plugin-commands-rebuild@7.1.3
  - @pnpm/store-connection-manager@5.2.16
  - @pnpm/find-workspace-packages@5.0.38
  - @pnpm/filter-workspace-packages@6.0.38

## 11.5.2

### Patch Changes

- Updated dependencies [7d64d757b]
- Updated dependencies [5c31fa8be]
  - @pnpm/cli-utils@1.1.2
  - @pnpm/core@7.9.0
  - @pnpm/config@16.7.0
  - @pnpm/plugin-commands-rebuild@7.1.2
  - @pnpm/find-workspace-packages@5.0.37
  - @pnpm/pnpmfile@4.0.35
  - @pnpm/store-connection-manager@5.2.15
  - @pnpm/filter-workspace-packages@6.0.37

## 11.5.1

### Patch Changes

- Updated dependencies [ed946c73e]
  - @pnpm/core@7.8.4
  - @pnpm/outdated@11.0.20
  - @pnpm/pnpmfile@4.0.34
  - @pnpm/plugin-commands-rebuild@7.1.1
  - @pnpm/config@16.6.4
  - @pnpm/cli-utils@1.1.1
  - @pnpm/store-connection-manager@5.2.14
  - @pnpm/find-workspace-packages@5.0.36
  - @pnpm/filter-workspace-packages@6.0.36

## 11.5.0

### Minor Changes

- 0377d9367: Add --report-summary for pnpm exec and pnpm run [#6008](https://github.com/pnpm/pnpm/issues/6008)

### Patch Changes

- Updated dependencies [0377d9367]
- Updated dependencies [972de58ab]
  - @pnpm/plugin-commands-rebuild@7.1.0
  - @pnpm/cli-utils@1.1.0
  - @pnpm/core@7.8.3
  - @pnpm/find-workspace-packages@5.0.35
  - @pnpm/pnpmfile@4.0.33
  - @pnpm/filter-workspace-packages@6.0.35
  - @pnpm/config@16.6.3
  - @pnpm/store-connection-manager@5.2.13

## 11.4.6

### Patch Changes

- Updated dependencies [f17ca4218]
  - @pnpm/core@7.8.2
  - @pnpm/pnpmfile@4.0.32
  - @pnpm/outdated@11.0.19
  - @pnpm/package-store@15.1.7
  - @pnpm/store-connection-manager@5.2.12
  - @pnpm/config@16.6.2
  - @pnpm/plugin-commands-rebuild@7.0.34
  - @pnpm/cli-utils@1.0.34
  - @pnpm/find-workspace-packages@5.0.34
  - @pnpm/filter-workspace-packages@6.0.34

## 11.4.5

### Patch Changes

- Updated dependencies [029143cff]
- Updated dependencies [029143cff]
  - @pnpm/resolver-base@9.2.0
  - @pnpm/core@7.8.1
  - @pnpm/package-store@15.1.7
  - @pnpm/config@16.6.1
  - @pnpm/plugin-commands-rebuild@7.0.33
  - @pnpm/outdated@11.0.18
  - @pnpm/pnpmfile@4.0.31
  - @pnpm/store-connection-manager@5.2.11
  - @pnpm/cli-utils@1.0.33
  - @pnpm/find-workspace-packages@5.0.33
  - @pnpm/filter-workspace-packages@6.0.33

## 11.4.4

### Patch Changes

- 308eb2c9b: Use Map rather than Object in `createPackageExtender` to prevent read the prototype property to native function
- Updated dependencies [74b535f19]
- Updated dependencies [308eb2c9b]
- Updated dependencies [59ee53678]
  - @pnpm/core@7.8.0
  - @pnpm/config@16.6.0
  - @pnpm/plugin-commands-rebuild@7.0.32
  - @pnpm/outdated@11.0.17
  - @pnpm/pnpmfile@4.0.30
  - @pnpm/cli-utils@1.0.32
  - @pnpm/store-connection-manager@5.2.10
  - @pnpm/package-store@15.1.6
  - @pnpm/find-workspace-packages@5.0.32
  - @pnpm/filter-workspace-packages@6.0.32

## 11.4.3

### Patch Changes

- 6348f5931: The update command should not replace dependency versions specified via dist-tags [#5996](https://github.com/pnpm/pnpm/pull/5996).
  - @pnpm/plugin-commands-rebuild@7.0.31
  - @pnpm/core@7.7.3
  - @pnpm/outdated@11.0.16
  - @pnpm/pnpmfile@4.0.29
  - @pnpm/package-store@15.1.5
  - @pnpm/config@16.5.5
  - @pnpm/store-connection-manager@5.2.9
  - @pnpm/cli-utils@1.0.31
  - @pnpm/find-workspace-packages@5.0.31
  - @pnpm/filter-workspace-packages@6.0.31

## 11.4.2

### Patch Changes

- @pnpm/plugin-commands-rebuild@7.0.30
- @pnpm/core@7.7.2
- @pnpm/package-store@15.1.4
- @pnpm/pnpmfile@4.0.28
- @pnpm/store-connection-manager@5.2.8
- @pnpm/config@16.5.4
- @pnpm/cli-utils@1.0.30
- @pnpm/find-workspace-packages@5.0.30
- @pnpm/filter-workspace-packages@6.0.30

## 11.4.1

### Patch Changes

- @pnpm/core@7.7.1
- @pnpm/pnpmfile@4.0.27
- @pnpm/config@16.5.3
- @pnpm/cli-utils@1.0.29
- @pnpm/plugin-commands-rebuild@7.0.29
- @pnpm/store-connection-manager@5.2.7
- @pnpm/find-workspace-packages@5.0.29
- @pnpm/filter-workspace-packages@6.0.29

## 11.4.0

### Minor Changes

- e8f6ab683: Add a `pnpm dedupe` command that removes dependencies from the lockfile by re-resolving the dependency graph. This work similar to yarn's [`yarn dedupe --strategy highest`](https://yarnpkg.com/cli/dedupe) command.

### Patch Changes

- Updated dependencies [e8f6ab683]
- Updated dependencies [1072ec128]
  - @pnpm/core@7.7.0
  - @pnpm/pnpmfile@4.0.26
  - @pnpm/config@16.5.2
  - @pnpm/cli-utils@1.0.28
  - @pnpm/plugin-commands-rebuild@7.0.28
  - @pnpm/store-connection-manager@5.2.6
  - @pnpm/find-workspace-packages@5.0.28
  - @pnpm/filter-workspace-packages@6.0.28

## 11.3.5

### Patch Changes

- 4655dd41e: Add more info to the description of the `--force` option in the `pnpm install` command.
  - @pnpm/core@7.6.5
  - @pnpm/package-store@15.1.3
  - @pnpm/plugin-commands-rebuild@7.0.27
  - @pnpm/store-connection-manager@5.2.5
  - @pnpm/pnpmfile@4.0.25
  - @pnpm/config@16.5.1
  - @pnpm/cli-utils@1.0.27
  - @pnpm/find-workspace-packages@5.0.27
  - @pnpm/filter-workspace-packages@6.0.27

## 11.3.4

### Patch Changes

- Updated dependencies [2ae1c449d]
- Updated dependencies [28b47a156]
  - @pnpm/parse-wanted-dependency@4.1.0
  - @pnpm/config@16.5.0
  - @pnpm/core@7.6.4
  - @pnpm/plugin-commands-rebuild@7.0.26
  - @pnpm/cli-utils@1.0.26
  - @pnpm/store-connection-manager@5.2.4
  - @pnpm/pnpmfile@4.0.24
  - @pnpm/find-workspace-packages@5.0.26
  - @pnpm/filter-workspace-packages@6.0.26

## 11.3.3

### Patch Changes

- Updated dependencies [1e6de89b6]
- Updated dependencies [9d425962f]
  - @pnpm/package-store@15.1.2
  - @pnpm/core@7.6.3
  - @pnpm/outdated@11.0.15
  - @pnpm/store-connection-manager@5.2.3
  - @pnpm/pnpmfile@4.0.23
  - @pnpm/plugin-commands-rebuild@7.0.25
  - @pnpm/config@16.4.3
  - @pnpm/cli-utils@1.0.25
  - @pnpm/find-workspace-packages@5.0.25
  - @pnpm/filter-workspace-packages@6.0.25

## 11.3.2

### Patch Changes

- @pnpm/core@7.6.2
- @pnpm/outdated@11.0.14
- @pnpm/pnpmfile@4.0.22
- @pnpm/plugin-commands-rebuild@7.0.24
- @pnpm/config@16.4.2
- @pnpm/cli-utils@1.0.24
- @pnpm/store-connection-manager@5.2.2
- @pnpm/find-workspace-packages@5.0.24
- @pnpm/filter-workspace-packages@6.0.24

## 11.3.1

### Patch Changes

- @pnpm/plugin-commands-rebuild@7.0.23
- @pnpm/core@7.6.1
- @pnpm/outdated@11.0.13
- @pnpm/pnpmfile@4.0.21
- @pnpm/package-store@15.1.1
- @pnpm/config@16.4.1
- @pnpm/store-connection-manager@5.2.1
- @pnpm/cli-utils@1.0.23
- @pnpm/find-workspace-packages@5.0.23
- @pnpm/filter-workspace-packages@6.0.23

## 11.3.0

### Minor Changes

- 3ebce5db7: Added support for `pnpm-lock.yaml` format v6. This new format will be the new lockfile format in pnpm v8. To use the new lockfile format, use the `use-lockfile-v6=true` setting in `.npmrc`. Or run `pnpm install --use-lockfile-v6` [#5810](https://github.com/pnpm/pnpm/pull/5810).

### Patch Changes

- Updated dependencies [891a8d763]
- Updated dependencies [c7b05cd9a]
- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
  - @pnpm/package-store@15.1.0
  - @pnpm/store-connection-manager@5.2.0
  - @pnpm/core@7.6.0
  - @pnpm/constants@6.2.0
  - @pnpm/config@16.4.0
  - @pnpm/plugin-commands-rebuild@7.0.22
  - @pnpm/pnpmfile@4.0.20
  - @pnpm/outdated@11.0.12
  - @pnpm/error@4.0.1
  - @pnpm/find-workspace-packages@5.0.22
  - @pnpm/cli-utils@1.0.22
  - @pnpm/manifest-utils@4.1.4
  - @pnpm/read-project-manifest@4.1.3
  - @pnpm/filter-workspace-packages@6.0.22
  - @pnpm/find-workspace-dir@5.0.1

## 11.2.0

### Minor Changes

- 1fad508b0: When the `resolve-peers-from-workspace-root` setting is set to `true`, pnpm will use dependencies installed in the root of the workspace to resolve peer dependencies in any of the workspace's projects [#5882](https://github.com/pnpm/pnpm/pull/5882).

### Patch Changes

- Updated dependencies [1fad508b0]
  - @pnpm/core@7.5.0
  - @pnpm/config@16.3.0
  - @pnpm/pnpmfile@4.0.19
  - @pnpm/cli-utils@1.0.21
  - @pnpm/plugin-commands-rebuild@7.0.21
  - @pnpm/store-connection-manager@5.1.14
  - @pnpm/find-workspace-packages@5.0.21
  - @pnpm/filter-workspace-packages@6.0.21

## 11.1.7

### Patch Changes

- 08ceaf3fc: replace dependency `is-ci` by `ci-info` (`is-ci` is just a simple wrapper around `ci-info`).
- Updated dependencies [ec97a3105]
- Updated dependencies [08ceaf3fc]
  - @pnpm/store-connection-manager@5.1.13
  - @pnpm/filter-workspace-packages@6.0.20
  - @pnpm/core@7.4.1
  - @pnpm/plugin-commands-rebuild@7.0.20
  - @pnpm/cli-utils@1.0.20
  - @pnpm/outdated@11.0.11
  - @pnpm/package-store@15.0.5
  - @pnpm/pnpmfile@4.0.18
  - @pnpm/find-workspace-packages@5.0.20
  - @pnpm/config@16.2.2

## 11.1.6

### Patch Changes

- d71dbf230: Only the `pnpm add --global <pkg>` command should fail if there is no global pnpm bin directory in the system PATH [#5841](https://github.com/pnpm/pnpm/issues/5841).
- Updated dependencies [d71dbf230]
  - @pnpm/config@16.2.1
  - @pnpm/cli-utils@1.0.19
  - @pnpm/plugin-commands-rebuild@7.0.19
  - @pnpm/store-connection-manager@5.1.12
  - @pnpm/find-workspace-packages@5.0.19
  - @pnpm/filter-workspace-packages@6.0.19

## 11.1.5

### Patch Changes

- Updated dependencies [5c75ff3ec]
- Updated dependencies [5c75ff3ec]
- Updated dependencies [841f52e70]
  - @pnpm/plugin-commands-rebuild@7.0.18
  - @pnpm/config@16.2.0
  - @pnpm/core@7.4.0
  - @pnpm/outdated@11.0.10
  - @pnpm/package-store@15.0.5
  - @pnpm/store-connection-manager@5.1.11
  - @pnpm/cli-utils@1.0.18
  - @pnpm/find-workspace-packages@5.0.18
  - @pnpm/filter-workspace-packages@6.0.18

## 11.1.4

### Patch Changes

- Updated dependencies [c9d3970e3]
- Updated dependencies [b77651d14]
- Updated dependencies [2458741fa]
  - @pnpm/plugin-commands-rebuild@7.0.17
  - @pnpm/types@8.10.0
  - @pnpm/core@7.4.0
  - @pnpm/cli-utils@1.0.17
  - @pnpm/config@16.1.11
  - @pnpm/pnpmfile@4.0.17
  - @pnpm/manifest-utils@4.1.3
  - @pnpm/read-project-manifest@4.1.2
  - @pnpm/resolver-base@9.1.5
  - @pnpm/outdated@11.0.9
  - @pnpm/package-store@15.0.5
  - @pnpm/find-workspace-packages@5.0.17
  - @pnpm/sort-packages@4.0.3
  - @pnpm/store-connection-manager@5.1.10
  - @pnpm/filter-workspace-packages@6.0.17

## 11.1.3

### Patch Changes

- Updated dependencies [450e0b1d1]
  - @pnpm/core@7.3.0
  - @pnpm/filter-workspace-packages@6.0.16
  - @pnpm/pnpmfile@4.0.16
  - @pnpm/plugin-commands-rebuild@7.0.16
  - @pnpm/outdated@11.0.8
  - @pnpm/find-workspace-packages@5.0.16
  - @pnpm/config@16.1.10
  - @pnpm/package-store@15.0.4
  - @pnpm/cli-utils@1.0.16
  - @pnpm/store-connection-manager@5.1.9

## 11.1.2

### Patch Changes

- 49f6c917f: `pnpm update` should not replace `workspace:*`, `workspace:~`, and `workspace:^` with `workspace:<version>` [#5764](https://github.com/pnpm/pnpm/pull/5764).
- Updated dependencies [49f6c917f]
  - @pnpm/core@7.2.5
  - @pnpm/pnpmfile@4.0.15
  - @pnpm/plugin-commands-rebuild@7.0.15
  - @pnpm/config@16.1.9
  - @pnpm/cli-utils@1.0.15
  - @pnpm/store-connection-manager@5.1.8
  - @pnpm/find-workspace-packages@5.0.15
  - @pnpm/filter-workspace-packages@6.0.15

## 11.1.1

### Patch Changes

- @pnpm/cli-utils@1.0.14
- @pnpm/core@7.2.4
- @pnpm/plugin-commands-rebuild@7.0.14
- @pnpm/find-workspace-packages@5.0.14
- @pnpm/pnpmfile@4.0.14
- @pnpm/filter-workspace-packages@6.0.14
- @pnpm/config@16.1.8
- @pnpm/store-connection-manager@5.1.7

## 11.1.0

### Minor Changes

- 4097af6b5: Overrides may be defined as a reference to a spec for a direct dependency by prefixing the name of the package you wish the version to match with a `# @pnpm/plugin-commands-installation.

  ```json
  {
    "dependencies": {
      "foo": "^1.0.0"
    },
    "overrides": {
      // the override is defined as a reference to the dependency
      "foo": "$foo",
      // the referenced package does not need to match the overridden one
      "bar": "$foo"
    }
  }
  ```

### Patch Changes

- Updated dependencies [c245edf1b]
- Updated dependencies [a9d59d8bc]
  - @pnpm/manifest-utils@4.1.2
  - @pnpm/config@16.1.7
  - @pnpm/parse-wanted-dependency@4.0.1
  - @pnpm/package-store@15.0.3
  - @pnpm/cli-utils@1.0.13
  - @pnpm/core@7.2.3
  - @pnpm/outdated@11.0.7
  - @pnpm/plugin-commands-rebuild@7.0.13
  - @pnpm/store-connection-manager@5.1.6
  - @pnpm/read-project-manifest@4.1.1
  - @pnpm/find-workspace-packages@5.0.13
  - @pnpm/pnpmfile@4.0.13
  - @pnpm/filter-workspace-packages@6.0.13

## 11.0.12

### Patch Changes

- @pnpm/core@7.2.2
- @pnpm/pnpmfile@4.0.12
- @pnpm/config@16.1.6
- @pnpm/cli-utils@1.0.12
- @pnpm/plugin-commands-rebuild@7.0.12
- @pnpm/store-connection-manager@5.1.5
- @pnpm/find-workspace-packages@5.0.12
- @pnpm/filter-workspace-packages@6.0.12

## 11.0.11

### Patch Changes

- @pnpm/core@7.2.1
- @pnpm/pnpmfile@4.0.11
- @pnpm/config@16.1.5
- @pnpm/cli-utils@1.0.11
- @pnpm/plugin-commands-rebuild@7.0.11
- @pnpm/store-connection-manager@5.1.4
- @pnpm/find-workspace-packages@5.0.11
- @pnpm/filter-workspace-packages@6.0.11

## 11.0.10

### Patch Changes

- 868f2fb16: readPackage hooks should not modify the `package.json` files in a workspace [#5670](https://github.com/pnpm/pnpm/issues/5670).
- Updated dependencies [043bbeaf3]
- Updated dependencies [fec9e3149]
- Updated dependencies [868f2fb16]
- Updated dependencies [0d12d38fd]
  - @pnpm/core@7.2.0
  - @pnpm/read-project-manifest@4.1.0
  - @pnpm/pnpmfile@4.0.10
  - @pnpm/cli-utils@1.0.10
  - @pnpm/config@16.1.4
  - @pnpm/plugin-commands-rebuild@7.0.10
  - @pnpm/find-workspace-packages@5.0.10
  - @pnpm/store-connection-manager@5.1.3
  - @pnpm/filter-workspace-packages@6.0.10
  - @pnpm/outdated@11.0.6
  - @pnpm/package-store@15.0.2

## 11.0.9

### Patch Changes

- 969f8a002: `pnpm update --latest !foo` should not update anything if the only dependency in the project is the ignored one [#5643](https://github.com/pnpm/pnpm/pull/5643).
- Updated dependencies [969f8a002]
  - @pnpm/matcher@4.0.1
  - @pnpm/core@7.1.1
  - @pnpm/config@16.1.3
  - @pnpm/outdated@11.0.5
  - @pnpm/filter-workspace-packages@6.0.9
  - @pnpm/pnpmfile@4.0.9
  - @pnpm/cli-utils@1.0.9
  - @pnpm/plugin-commands-rebuild@7.0.9
  - @pnpm/store-connection-manager@5.1.2
  - @pnpm/find-workspace-packages@5.0.9

## 11.0.8

### Patch Changes

- Updated dependencies [1d04e663b]
  - @pnpm/core@7.1.0
  - @pnpm/pnpmfile@4.0.8
  - @pnpm/plugin-commands-rebuild@7.0.8
  - @pnpm/config@16.1.2
  - @pnpm/cli-utils@1.0.8
  - @pnpm/store-connection-manager@5.1.1
  - @pnpm/find-workspace-packages@5.0.8
  - @pnpm/filter-workspace-packages@6.0.8

## 11.0.7

### Patch Changes

- Updated dependencies [eacff33e4]
  - @pnpm/store-connection-manager@5.1.0
  - @pnpm/core@7.0.7
  - @pnpm/outdated@11.0.4
  - @pnpm/package-store@15.0.2
  - @pnpm/plugin-commands-rebuild@7.0.7
  - @pnpm/pnpmfile@4.0.7
  - @pnpm/config@16.1.1
  - @pnpm/cli-utils@1.0.7
  - @pnpm/find-workspace-packages@5.0.7
  - @pnpm/filter-workspace-packages@6.0.7

## 11.0.6

### Patch Changes

- 3dab7f83c: Replace environment variable placeholders with their values, when reading `.npmrc` files in subdirectories inside a workspace [#2570](https://github.com/pnpm/pnpm/issues/2570).
- Updated dependencies [3dab7f83c]
- Updated dependencies [3dab7f83c]
  - @pnpm/config@16.1.0
  - @pnpm/plugin-commands-rebuild@7.0.6
  - @pnpm/cli-utils@1.0.6
  - @pnpm/store-connection-manager@5.0.6
  - @pnpm/core@7.0.6
  - @pnpm/find-workspace-packages@5.0.6
  - @pnpm/pnpmfile@4.0.6
  - @pnpm/filter-workspace-packages@6.0.6

## 11.0.5

### Patch Changes

- Updated dependencies [2e9790722]
- Updated dependencies [702e847c1]
  - @pnpm/find-workspace-packages@5.0.5
  - @pnpm/types@8.9.0
  - @pnpm/cli-utils@1.0.5
  - @pnpm/core@7.0.5
  - @pnpm/plugin-commands-rebuild@7.0.5
  - @pnpm/filter-workspace-packages@6.0.5
  - @pnpm/config@16.0.5
  - @pnpm/manifest-utils@4.1.1
  - @pnpm/outdated@11.0.3
  - @pnpm/package-store@15.0.2
  - @pnpm/pnpmfile@4.0.5
  - @pnpm/read-project-manifest@4.0.2
  - @pnpm/resolver-base@9.1.4
  - @pnpm/sort-packages@4.0.2
  - @pnpm/store-connection-manager@5.0.5

## 11.0.4

### Patch Changes

- 0da2f0412: Update dependencies.
  - @pnpm/core@7.0.4
  - @pnpm/pnpmfile@4.0.4
  - @pnpm/config@16.0.4
  - @pnpm/cli-utils@1.0.4
  - @pnpm/plugin-commands-rebuild@7.0.4
  - @pnpm/store-connection-manager@5.0.4
  - @pnpm/find-workspace-packages@5.0.4
  - @pnpm/filter-workspace-packages@6.0.4

## 11.0.3

### Patch Changes

- Updated dependencies [aacb83f73]
- Updated dependencies [a14ad09e6]
  - @pnpm/config@16.0.3
  - @pnpm/core@7.0.3
  - @pnpm/cli-utils@1.0.3
  - @pnpm/plugin-commands-rebuild@7.0.3
  - @pnpm/store-connection-manager@5.0.3
  - @pnpm/pnpmfile@4.0.3
  - @pnpm/find-workspace-packages@5.0.3
  - @pnpm/filter-workspace-packages@6.0.3

## 11.0.2

### Patch Changes

- Updated dependencies [bea0acdfc]
  - @pnpm/config@16.0.2
  - @pnpm/outdated@11.0.2
  - @pnpm/cli-utils@1.0.2
  - @pnpm/plugin-commands-rebuild@7.0.2
  - @pnpm/store-connection-manager@5.0.2
  - @pnpm/core@7.0.2
  - @pnpm/package-store@15.0.1
  - @pnpm/find-workspace-packages@5.0.2
  - @pnpm/pnpmfile@4.0.2
  - @pnpm/filter-workspace-packages@6.0.2

## 11.0.1

### Patch Changes

- Updated dependencies [e7fd8a84c]
- Updated dependencies [844e82f3a]
- Updated dependencies [844e82f3a]
- Updated dependencies [768d41745]
- Updated dependencies [844e82f3a]
  - @pnpm/config@16.0.1
  - @pnpm/types@8.8.0
  - @pnpm/core@7.0.1
  - @pnpm/outdated@11.0.1
  - @pnpm/manifest-utils@4.1.0
  - @pnpm/cli-utils@1.0.1
  - @pnpm/plugin-commands-rebuild@7.0.1
  - @pnpm/store-connection-manager@5.0.1
  - @pnpm/find-workspace-packages@5.0.1
  - @pnpm/package-store@15.0.1
  - @pnpm/pnpmfile@4.0.1
  - @pnpm/read-project-manifest@4.0.1
  - @pnpm/resolver-base@9.1.3
  - @pnpm/sort-packages@4.0.1
  - @pnpm/filter-workspace-packages@6.0.1

## 11.0.0

### Major Changes

- 645384bfd: New required option added: `allProjectsGraph`.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- e35988d1f: Update Yarn dependencies.
- Updated dependencies [043d988fc]
- Updated dependencies [1d0fd82fd]
- Updated dependencies [645384bfd]
- Updated dependencies [6ad990a6a]
- Updated dependencies [f884689e0]
- Updated dependencies [6ad990a6a]
- Updated dependencies [3c117996e]
- Updated dependencies [645384bfd]
- Updated dependencies [645384bfd]
  - @pnpm/config@16.0.0
  - @pnpm/error@4.0.0
  - @pnpm/filter-workspace-packages@6.0.0
  - @pnpm/find-workspace-dir@5.0.0
  - @pnpm/find-workspace-packages@5.0.0
  - @pnpm/outdated@11.0.0
  - @pnpm/cli-utils@1.0.0
  - @pnpm/core@7.0.0
  - @pnpm/manifest-utils@4.0.0
  - @pnpm/matcher@4.0.0
  - @pnpm/package-store@15.0.0
  - @pnpm/parse-wanted-dependency@4.0.0
  - @pnpm/plugin-commands-rebuild@7.0.0
  - @pnpm/pnpmfile@4.0.0
  - @pnpm/read-project-manifest@4.0.0
  - @pnpm/sort-packages@4.0.0
  - @pnpm/store-connection-manager@5.0.0

## 10.8.4

### Patch Changes

- 96b507b73: `pnpm link <pkg> --global` should work when a custom target directory is specified with the `--dir` CLI option [#5473](https://github.com/pnpm/pnpm/pull/5473).
- Updated dependencies [96b507b73]
  - @pnpm/core@6.0.3
  - @pnpm/store-connection-manager@4.3.16
  - @pnpm/pnpmfile@3.0.3
  - @pnpm/outdated@10.1.3
  - @pnpm/read-project-manifest@3.0.13
  - @pnpm/plugin-commands-rebuild@6.2.12
  - @pnpm/config@15.10.12
  - @pnpm/cli-utils@0.7.43
  - @pnpm/find-workspace-packages@4.0.43
  - @pnpm/filter-workspace-packages@5.1.3
  - @pnpm/package-store@14.2.7

## 10.8.3

### Patch Changes

- 3277188eb: Add global option in `pnpm link --help` [#5461](https://github.com/pnpm/pnpm/pull/5461).
  - @pnpm/core@6.0.2
  - @pnpm/manifest-utils@3.1.6
  - @pnpm/plugin-commands-rebuild@6.2.11
  - @pnpm/pnpmfile@3.0.2
  - @pnpm/cli-utils@0.7.42
  - @pnpm/outdated@10.1.2
  - @pnpm/package-store@14.2.7
  - @pnpm/config@15.10.11
  - @pnpm/find-workspace-packages@4.0.42
  - @pnpm/store-connection-manager@4.3.15
  - @pnpm/filter-workspace-packages@5.1.2

## 10.8.2

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/cli-utils@0.7.41
  - @pnpm/config@15.10.10
  - @pnpm/core@6.0.1
  - @pnpm/filter-workspace-packages@5.1.1
  - @pnpm/find-workspace-dir@4.0.3
  - @pnpm/manifest-utils@3.1.5
  - @pnpm/outdated@10.1.1
  - @pnpm/pnpmfile@3.0.1
  - @pnpm/read-project-manifest@3.0.12
  - @pnpm/store-connection-manager@4.3.14
  - @pnpm/plugin-commands-rebuild@6.2.10
  - @pnpm/find-workspace-packages@4.0.41
  - @pnpm/package-store@14.2.6

## 10.8.1

### Patch Changes

- 536b16856: `pnpm update --interactive` should not list dependencies ignored via the `pnpm.updateConfig.ignoreDependencies` setting.

## 10.8.0

### Minor Changes

- abb41a626: It is possible now to update all dependencies except the listed ones using `!`. For instance, update all dependencies, except `lodash`:

  ```
  pnpm update !lodash
  ```

  It also works with pattends, for instance:

  ```
  pnpm update !@babel/*
  ```

  And it may be combined with other patterns:

  ```
  pnpm update @babel/* !@babel/core
  ```

- d665f3ff7: Ignore packages listed in package.json > pnpm.updateConfig.ignoreDependencies fields on update/outdated command [#5358](https://github.com/pnpm/pnpm/issues/5358)

### Patch Changes

- 51566e34b: Combining readPackage hook from options and from pnpmfile
- 5beb4e26b: fix dir path repeated join in link global
- Updated dependencies [abb41a626]
- Updated dependencies [51566e34b]
- Updated dependencies [2e830c0cb]
- Updated dependencies [51566e34b]
- Updated dependencies [d665f3ff7]
- Updated dependencies [51566e34b]
  - @pnpm/matcher@3.2.0
  - @pnpm/core@6.0.0
  - @pnpm/filter-workspace-packages@5.1.0
  - @pnpm/outdated@10.1.0
  - @pnpm/types@8.7.0
  - @pnpm/pnpmfile@3.0.0
  - @pnpm/config@15.10.9
  - @pnpm/cli-utils@0.7.40
  - @pnpm/plugin-commands-rebuild@6.2.9
  - @pnpm/find-workspace-packages@4.0.40
  - @pnpm/manifest-utils@3.1.4
  - @pnpm/package-store@14.2.5
  - @pnpm/read-project-manifest@3.0.11
  - @pnpm/resolver-base@9.1.2
  - @pnpm/sort-packages@3.0.9
  - @pnpm/store-connection-manager@4.3.13

## 10.7.2

### Patch Changes

- 56aeba4ba: add path join and add test case for pnpm --dir <path> link global
- 56aeba4ba: fix error in pnpm --dir <path> link --global
  - @pnpm/core@5.12.2
  - @pnpm/pnpmfile@2.2.12
  - @pnpm/config@15.10.8
  - @pnpm/cli-utils@0.7.39
  - @pnpm/plugin-commands-rebuild@6.2.8
  - @pnpm/store-connection-manager@4.3.12
  - @pnpm/find-workspace-packages@4.0.39
  - @pnpm/filter-workspace-packages@5.0.39

## 10.7.1

### Patch Changes

- @pnpm/core@5.12.1
- @pnpm/pnpmfile@2.2.11
- @pnpm/config@15.10.7
- @pnpm/cli-utils@0.7.38
- @pnpm/plugin-commands-rebuild@6.2.7
- @pnpm/store-connection-manager@4.3.11
- @pnpm/find-workspace-packages@4.0.38
- @pnpm/filter-workspace-packages@5.0.38

## 10.7.0

### Minor Changes

- 156cc1ef6: A new setting supported in the pnpm section of the `package.json` file: `allowNonAppliedPatches`. When it is set to `true`, non-applied patches will not cause an error, just a warning will be printed. For example:

  ```json
  {
    "name": "foo",
    "version": "1.0.0",
    "pnpm": {
      "patchedDependencies": {
        "express@4.18.1": "patches/express@4.18.1.patch"
      },
      "allowNonAppliedPatches": true
    }
  }
  ```

### Patch Changes

- Updated dependencies [156cc1ef6]
- Updated dependencies [9b44d38a4]
  - @pnpm/core@5.12.0
  - @pnpm/types@8.6.0
  - @pnpm/matcher@3.1.0
  - @pnpm/pnpmfile@2.2.10
  - @pnpm/cli-utils@0.7.37
  - @pnpm/config@15.10.6
  - @pnpm/find-workspace-packages@4.0.37
  - @pnpm/manifest-utils@3.1.3
  - @pnpm/outdated@10.0.28
  - @pnpm/package-store@14.2.4
  - @pnpm/plugin-commands-rebuild@6.2.6
  - @pnpm/read-project-manifest@3.0.10
  - @pnpm/resolver-base@9.1.1
  - @pnpm/sort-packages@3.0.8
  - @pnpm/filter-workspace-packages@5.0.37
  - @pnpm/store-connection-manager@4.3.10

## 10.6.5

### Patch Changes

- @pnpm/core@5.11.5
- @pnpm/outdated@10.0.27
- @pnpm/package-store@14.2.3
- @pnpm/store-connection-manager@4.3.9
- @pnpm/pnpmfile@2.2.9
- @pnpm/plugin-commands-rebuild@6.2.5
- @pnpm/config@15.10.5
- @pnpm/cli-utils@0.7.36
- @pnpm/find-workspace-packages@4.0.36
- @pnpm/filter-workspace-packages@5.0.36

## 10.6.4

### Patch Changes

- Updated dependencies [2acf38be3]
  - @pnpm/core@5.11.4
  - @pnpm/pnpmfile@2.2.8
  - @pnpm/cli-utils@0.7.35
  - @pnpm/config@15.10.4
  - @pnpm/find-workspace-packages@4.0.35
  - @pnpm/plugin-commands-rebuild@6.2.4
  - @pnpm/store-connection-manager@4.3.8
  - @pnpm/filter-workspace-packages@5.0.35

## 10.6.3

### Patch Changes

- @pnpm/core@5.11.3
- @pnpm/outdated@10.0.26
- @pnpm/pnpmfile@2.2.7
- @pnpm/plugin-commands-rebuild@6.2.3
- @pnpm/package-store@14.2.3
- @pnpm/store-connection-manager@4.3.7
- @pnpm/config@15.10.3
- @pnpm/cli-utils@0.7.34
- @pnpm/find-workspace-packages@4.0.34
- @pnpm/filter-workspace-packages@5.0.34

## 10.6.2

### Patch Changes

- @pnpm/core@5.11.2
- @pnpm/pnpmfile@2.2.6
- @pnpm/outdated@10.0.25
- @pnpm/package-store@14.2.3
- @pnpm/store-connection-manager@4.3.6
- @pnpm/config@15.10.2
- @pnpm/plugin-commands-rebuild@6.2.2
- @pnpm/cli-utils@0.7.33
- @pnpm/find-workspace-packages@4.0.33
- @pnpm/filter-workspace-packages@5.0.33

## 10.6.1

### Patch Changes

- Updated dependencies [17e69e18b]
  - @pnpm/package-store@14.2.3
  - @pnpm/store-connection-manager@4.3.5
  - @pnpm/core@5.11.1
  - @pnpm/plugin-commands-rebuild@6.2.1
  - @pnpm/pnpmfile@2.2.5
  - @pnpm/outdated@10.0.24
  - @pnpm/config@15.10.1
  - @pnpm/cli-utils@0.7.32
  - @pnpm/find-workspace-packages@4.0.32
  - @pnpm/filter-workspace-packages@5.0.32

## 10.6.0

### Minor Changes

- 2aa22e4b1: Set `NODE_PATH` when `preferSymlinkedExecutables` is enabled.

### Patch Changes

- Updated dependencies [2aa22e4b1]
  - @pnpm/config@15.10.0
  - @pnpm/core@5.11.0
  - @pnpm/plugin-commands-rebuild@6.2.0
  - @pnpm/cli-utils@0.7.31
  - @pnpm/store-connection-manager@4.3.4
  - @pnpm/pnpmfile@2.2.4
  - @pnpm/find-workspace-packages@4.0.31
  - @pnpm/filter-workspace-packages@5.0.31

## 10.5.8

### Patch Changes

- Updated dependencies [f4cc2d7b4]
  - @pnpm/core@5.10.3
  - @pnpm/pnpmfile@2.2.3
  - @pnpm/config@15.9.4
  - @pnpm/cli-utils@0.7.30
  - @pnpm/plugin-commands-rebuild@6.1.34
  - @pnpm/store-connection-manager@4.3.3
  - @pnpm/find-workspace-packages@4.0.30
  - @pnpm/filter-workspace-packages@5.0.30

## 10.5.7

### Patch Changes

- @pnpm/core@5.10.2
- @pnpm/package-store@14.2.2
- @pnpm/store-connection-manager@4.3.2
- @pnpm/pnpmfile@2.2.2
- @pnpm/plugin-commands-rebuild@6.1.33
- @pnpm/config@15.9.3
- @pnpm/cli-utils@0.7.29
- @pnpm/find-workspace-packages@4.0.29
- @pnpm/filter-workspace-packages@5.0.29

## 10.5.6

### Patch Changes

- 9faf0221d: Update Yarn dependencies.
- Updated dependencies [dbac0ca01]
- Updated dependencies [9faf0221d]
  - @pnpm/package-store@14.2.1
  - @pnpm/core@5.10.1
  - @pnpm/store-connection-manager@4.3.1
  - @pnpm/plugin-commands-rebuild@6.1.32
  - @pnpm/pnpmfile@2.2.1
  - @pnpm/outdated@10.0.23
  - @pnpm/config@15.9.2
  - @pnpm/cli-utils@0.7.28
  - @pnpm/find-workspace-packages@4.0.28
  - @pnpm/filter-workspace-packages@5.0.28

## 10.5.5

### Patch Changes

- Updated dependencies [5035fdae1]
- Updated dependencies [23984abd1]
  - @pnpm/core@5.10.0
  - @pnpm/pnpmfile@2.2.0
  - @pnpm/package-store@14.2.0
  - @pnpm/resolver-base@9.1.0
  - @pnpm/store-connection-manager@4.3.0
  - @pnpm/plugin-commands-rebuild@6.1.31
  - @pnpm/config@15.9.1
  - @pnpm/outdated@10.0.22
  - @pnpm/cli-utils@0.7.27
  - @pnpm/find-workspace-packages@4.0.27
  - @pnpm/filter-workspace-packages@5.0.27

## 10.5.4

### Patch Changes

- Updated dependencies [238a165a5]
  - @pnpm/parse-wanted-dependency@3.0.2
  - @pnpm/core@5.9.1
  - @pnpm/package-store@14.1.1
  - @pnpm/outdated@10.0.21
  - @pnpm/store-connection-manager@4.2.1
  - @pnpm/plugin-commands-rebuild@6.1.30
  - @pnpm/config@15.9.0

## 10.5.3

### Patch Changes

- 39c040127: upgrade various dependencies
- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [39c040127]
- Updated dependencies [43cd6aaca]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
- Updated dependencies [29a81598a]
  - @pnpm/read-project-manifest@3.0.9
  - @pnpm/config@15.9.0
  - @pnpm/core@5.9.0
  - @pnpm/filter-workspace-packages@5.0.26
  - @pnpm/outdated@10.0.20
  - @pnpm/package-store@14.1.0
  - @pnpm/plugin-commands-rebuild@6.1.29
  - @pnpm/pnpmfile@2.1.0
  - @pnpm/store-connection-manager@4.2.0
  - @pnpm/cli-utils@0.7.26
  - @pnpm/find-workspace-packages@4.0.26

## 10.5.2

### Patch Changes

- Updated dependencies [c90798461]
- Updated dependencies [34121d753]
  - @pnpm/types@8.5.0
  - @pnpm/config@15.8.1
  - @pnpm/core@5.8.4
  - @pnpm/outdated@10.0.19
  - @pnpm/cli-utils@0.7.25
  - @pnpm/find-workspace-packages@4.0.25
  - @pnpm/manifest-utils@3.1.2
  - @pnpm/package-store@14.0.7
  - @pnpm/plugin-commands-rebuild@6.1.28
  - @pnpm/pnpmfile@2.0.9
  - @pnpm/read-project-manifest@3.0.8
  - @pnpm/resolver-base@9.0.6
  - @pnpm/sort-packages@3.0.7
  - @pnpm/store-connection-manager@4.1.26
  - @pnpm/filter-workspace-packages@5.0.25

## 10.5.1

### Patch Changes

- Updated dependencies [c7d65fe7f]
  - @pnpm/core@5.8.3
  - @pnpm/outdated@10.0.18
  - @pnpm/plugin-commands-rebuild@6.1.27

## 10.5.0

### Minor Changes

- cac34ad69: `verify-store-integrity=false` makes pnpm skip checking the integrities of files in the global content-addressable store.
- 99019e071: Allow to set `only-built-dependencies[]` through `.npmrc`.

### Patch Changes

- Updated dependencies [cac34ad69]
- Updated dependencies [99019e071]
  - @pnpm/config@15.8.0
  - @pnpm/cli-utils@0.7.24
  - @pnpm/plugin-commands-rebuild@6.1.26
  - @pnpm/store-connection-manager@4.1.25
  - @pnpm/core@5.8.2
  - @pnpm/package-store@14.0.6
  - @pnpm/find-workspace-packages@4.0.24
  - @pnpm/filter-workspace-packages@5.0.24

## 10.4.2

### Patch Changes

- @pnpm/core@5.8.1
- @pnpm/outdated@10.0.17
- @pnpm/pnpmfile@2.0.8
- @pnpm/plugin-commands-rebuild@6.1.25
- @pnpm/config@15.7.1
- @pnpm/cli-utils@0.7.23
- @pnpm/store-connection-manager@4.1.24
- @pnpm/package-store@14.0.5
- @pnpm/find-workspace-packages@4.0.23
- @pnpm/filter-workspace-packages@5.0.23

## 10.4.1

### Patch Changes

- Updated dependencies [01c5834bf]
- Updated dependencies [4fa1091c8]
  - @pnpm/read-project-manifest@3.0.7
  - @pnpm/config@15.7.0
  - @pnpm/core@5.8.0
  - @pnpm/cli-utils@0.7.22
  - @pnpm/plugin-commands-rebuild@6.1.24
  - @pnpm/store-connection-manager@4.1.23
  - @pnpm/outdated@10.0.16
  - @pnpm/find-workspace-packages@4.0.22
  - @pnpm/filter-workspace-packages@5.0.22
  - @pnpm/package-store@14.0.5

## 10.4.0

### Minor Changes

- 0569f1022: When `saveLockfile` is set to `false`, no changes to `pnpm-lock.yaml` are written to the filesystem.

### Patch Changes

- Updated dependencies [e3f4d131c]
- Updated dependencies [7334b347b]
- Updated dependencies [e3f4d131c]
- Updated dependencies [0569f1022]
  - @pnpm/core@5.7.0
  - @pnpm/config@15.6.1
  - @pnpm/manifest-utils@3.1.1
  - @pnpm/cli-utils@0.7.21
  - @pnpm/plugin-commands-rebuild@6.1.23
  - @pnpm/store-connection-manager@4.1.22
  - @pnpm/outdated@10.0.15
  - @pnpm/find-workspace-packages@4.0.21
  - @pnpm/filter-workspace-packages@5.0.21

## 10.3.10

### Patch Changes

- 406656f80: When `lockfile-include-tarball-url` is set to `true`, every entry in `pnpm-lock.yaml` will contain the full URL to the package's tarball [#5054](https://github.com/pnpm/pnpm/pull/5054).
- Updated dependencies [0ee3dfbe0]
- Updated dependencies [28f000509]
- Updated dependencies [406656f80]
- Updated dependencies [13210db3c]
  - @pnpm/core@5.6.0
  - @pnpm/config@15.6.0
  - @pnpm/plugin-commands-rebuild@6.1.22
  - @pnpm/cli-utils@0.7.20
  - @pnpm/store-connection-manager@4.1.21
  - @pnpm/outdated@10.0.14
  - @pnpm/package-store@14.0.5
  - @pnpm/find-workspace-packages@4.0.20
  - @pnpm/filter-workspace-packages@5.0.20

## 10.3.9

### Patch Changes

- Updated dependencies [744d47d90]
  - @pnpm/pnpmfile@2.0.7
  - @pnpm/config@15.5.2
  - @pnpm/cli-utils@0.7.19
  - @pnpm/plugin-commands-rebuild@6.1.21
  - @pnpm/store-connection-manager@4.1.20
  - @pnpm/find-workspace-packages@4.0.19
  - @pnpm/core@5.5.9
  - @pnpm/filter-workspace-packages@5.0.19

## 10.3.8

### Patch Changes

- Updated dependencies [d89bb43f2]
  - @pnpm/core@5.5.8

## 10.3.7

### Patch Changes

- Updated dependencies [f5621a42c]
- Updated dependencies [ff7061929]
  - @pnpm/manifest-utils@3.1.0
  - @pnpm/core@5.5.7
  - @pnpm/cli-utils@0.7.18
  - @pnpm/outdated@10.0.13
  - @pnpm/find-workspace-packages@4.0.18
  - @pnpm/plugin-commands-rebuild@6.1.20
  - @pnpm/filter-workspace-packages@5.0.18
  - @pnpm/package-store@14.0.5
  - @pnpm/store-connection-manager@4.1.19

## 10.3.6

### Patch Changes

- b55b3782d: Never skip lockfile resolution when the lockfile is not up-to-date and `--lockfile-only` is used. Even if `frozen-lockfile` is `true` [#4951](https://github.com/pnpm/pnpm/issues/4951).
- Updated dependencies [b55b3782d]
  - @pnpm/core@5.5.6
  - @pnpm/outdated@10.0.12
  - @pnpm/plugin-commands-rebuild@6.1.19
  - @pnpm/package-store@14.0.4
  - @pnpm/store-connection-manager@4.1.18

## 10.3.5

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
- Updated dependencies [42c1ea1c0]
- Updated dependencies [c5fdc5f35]
  - @pnpm/cli-utils@0.7.17
  - @pnpm/config@15.5.1
  - @pnpm/core@5.5.5
  - @pnpm/filter-workspace-packages@5.0.17
  - @pnpm/outdated@10.0.11
  - @pnpm/package-store@14.0.4
  - @pnpm/plugin-commands-rebuild@6.1.18
  - @pnpm/parse-wanted-dependency@3.0.1
  - @pnpm/find-workspace-packages@4.0.17
  - @pnpm/store-connection-manager@4.1.17

## 10.3.4

### Patch Changes

- Updated dependencies [fc581d371]
  - @pnpm/core@5.5.4
  - @pnpm/outdated@10.0.10
  - @pnpm/plugin-commands-rebuild@6.1.17
  - @pnpm/package-store@14.0.3
  - @pnpm/store-connection-manager@4.1.16

## 10.3.3

### Patch Changes

- Updated dependencies [7922d6314]
  - @pnpm/core@5.5.3
  - @pnpm/package-store@14.0.2
  - @pnpm/store-connection-manager@4.1.15
  - @pnpm/plugin-commands-rebuild@6.1.16

## 10.3.2

### Patch Changes

- Updated dependencies [f48d46ef6]
- Updated dependencies [12aa1e2e1]
- Updated dependencies [6434a8291]
  - @pnpm/config@15.5.0
  - @pnpm/core@5.5.2
  - @pnpm/find-workspace-dir@4.0.2
  - @pnpm/cli-utils@0.7.16
  - @pnpm/plugin-commands-rebuild@6.1.15
  - @pnpm/store-connection-manager@4.1.14
  - @pnpm/find-workspace-packages@4.0.16
  - @pnpm/filter-workspace-packages@5.0.16

## 10.3.1

### Patch Changes

- Updated dependencies [8e5b77ef6]
- Updated dependencies [285ff09ba]
- Updated dependencies [8e5b77ef6]
  - @pnpm/core@5.5.1
  - @pnpm/types@8.4.0
  - @pnpm/pnpmfile@2.0.6
  - @pnpm/cli-utils@0.7.15
  - @pnpm/config@15.4.1
  - @pnpm/find-workspace-packages@4.0.15
  - @pnpm/manifest-utils@3.0.6
  - @pnpm/outdated@10.0.9
  - @pnpm/package-store@14.0.1
  - @pnpm/plugin-commands-rebuild@6.1.14
  - @pnpm/read-project-manifest@3.0.6
  - @pnpm/resolver-base@9.0.5
  - @pnpm/sort-packages@3.0.6
  - @pnpm/store-connection-manager@4.1.13
  - @pnpm/filter-workspace-packages@5.0.15

## 10.3.0

### Minor Changes

- 2a34b21ce: Dependencies patching is possible via the `pnpm.patchedDependencies` field of the `package.json`.
  To patch a package, the package name, exact version, and the relative path to the patch file should be specified. For instance:

  ```json
  {
    "pnpm": {
      "patchedDependencies": {
        "eslint@1.0.0": "./patches/eslint@1.0.0.patch"
      }
    }
  }
  ```

### Patch Changes

- 949e84ba8: Don't crash when `pnpm update --interactive` is cancelled with Ctrl+c.
- Updated dependencies [2a34b21ce]
- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/core@5.5.0
  - @pnpm/types@8.3.0
  - @pnpm/package-store@14.0.0
  - @pnpm/config@15.4.0
  - @pnpm/cli-utils@0.7.14
  - @pnpm/find-workspace-packages@4.0.14
  - @pnpm/manifest-utils@3.0.5
  - @pnpm/outdated@10.0.8
  - @pnpm/plugin-commands-rebuild@6.1.13
  - @pnpm/pnpmfile@2.0.5
  - @pnpm/read-project-manifest@3.0.5
  - @pnpm/resolver-base@9.0.4
  - @pnpm/sort-packages@3.0.5
  - @pnpm/store-connection-manager@4.1.12
  - @pnpm/filter-workspace-packages@5.0.14

## 10.2.0

### Minor Changes

- 56cf04cb3: New settings added: use-git-branch-lockfile, merge-git-branch-lockfiles, merge-git-branch-lockfiles-branch-pattern.

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [fb5bbfd7a]
- Updated dependencies [0abfe1718]
- Updated dependencies [56cf04cb3]
  - @pnpm/core@5.4.0
  - @pnpm/types@8.2.0
  - @pnpm/config@15.3.0
  - @pnpm/cli-utils@0.7.13
  - @pnpm/find-workspace-packages@4.0.13
  - @pnpm/manifest-utils@3.0.4
  - @pnpm/outdated@10.0.7
  - @pnpm/package-store@13.0.8
  - @pnpm/plugin-commands-rebuild@6.1.12
  - @pnpm/pnpmfile@2.0.4
  - @pnpm/read-project-manifest@3.0.4
  - @pnpm/resolver-base@9.0.3
  - @pnpm/sort-packages@3.0.4
  - @pnpm/store-connection-manager@4.1.11
  - @pnpm/filter-workspace-packages@5.0.13

## 10.1.1

### Patch Changes

- Updated dependencies [25798aad1]
  - @pnpm/config@15.2.1
  - @pnpm/store-connection-manager@4.1.10
  - @pnpm/cli-utils@0.7.12
  - @pnpm/plugin-commands-rebuild@6.1.11
  - @pnpm/find-workspace-packages@4.0.12
  - @pnpm/core@5.3.1
  - @pnpm/filter-workspace-packages@5.0.12

## 10.1.0

### Minor Changes

- 4d39e4a0c: A new setting is supported for ignoring specific deprecation messages: `pnpm.allowedDeprecatedVersions`. The setting should be provided in the `pnpm` section of the root `package.json` file. The below example will mute any deprecation warnings about the `request` package and warnings about `express` v1:

  ```json
  {
    "pnpm": {
      "allowedDeprecatedVersions": {
        "request": "*",
        "express": "1"
      }
    }
  }
  ```

  Related issue: [#4306](https://github.com/pnpm/pnpm/issues/4306)
  Related PR: [#4864](https://github.com/pnpm/pnpm/pull/4864)

### Patch Changes

- Updated dependencies [4d39e4a0c]
- Updated dependencies [4d39e4a0c]
- Updated dependencies [c1238946f]
- Updated dependencies [bc80631d3]
- Updated dependencies [d5730ba81]
  - @pnpm/types@8.1.0
  - @pnpm/core@5.3.0
  - @pnpm/config@15.2.0
  - @pnpm/cli-utils@0.7.11
  - @pnpm/find-workspace-packages@4.0.11
  - @pnpm/manifest-utils@3.0.3
  - @pnpm/outdated@10.0.6
  - @pnpm/package-store@13.0.7
  - @pnpm/plugin-commands-rebuild@6.1.10
  - @pnpm/pnpmfile@2.0.3
  - @pnpm/read-project-manifest@3.0.3
  - @pnpm/resolver-base@9.0.2
  - @pnpm/sort-packages@3.0.3
  - @pnpm/store-connection-manager@4.1.9
  - @pnpm/filter-workspace-packages@5.0.11

## 10.0.12

### Patch Changes

- @pnpm/core@5.2.5

## 10.0.11

### Patch Changes

- Updated dependencies [6756c2b02]
  - @pnpm/core@5.2.4
  - @pnpm/cli-utils@0.7.10
  - @pnpm/package-store@13.0.6
  - @pnpm/plugin-commands-rebuild@6.1.9
  - @pnpm/find-workspace-packages@4.0.10
  - @pnpm/store-connection-manager@4.1.8
  - @pnpm/filter-workspace-packages@5.0.10
  - @pnpm/outdated@10.0.5
  - @pnpm/config@15.1.4

## 10.0.10

### Patch Changes

- @pnpm/core@5.2.3

## 10.0.9

### Patch Changes

- Updated dependencies [ae2f845c5]
  - @pnpm/config@15.1.4
  - @pnpm/cli-utils@0.7.9
  - @pnpm/plugin-commands-rebuild@6.1.8
  - @pnpm/store-connection-manager@4.1.7
  - @pnpm/core@5.2.2
  - @pnpm/find-workspace-packages@4.0.9
  - @pnpm/filter-workspace-packages@5.0.9

## 10.0.8

### Patch Changes

- Updated dependencies [05159665d]
  - @pnpm/config@15.1.3
  - @pnpm/cli-utils@0.7.8
  - @pnpm/plugin-commands-rebuild@6.1.7
  - @pnpm/store-connection-manager@4.1.6
  - @pnpm/find-workspace-packages@4.0.8
  - @pnpm/core@5.2.1
  - @pnpm/filter-workspace-packages@5.0.8

## 10.0.7

### Patch Changes

- 190f0b331: When `auto-install-peers` is set to `true`, automatically install missing peer dependencies without writing them to `package.json` as dependencies. This makes pnpm handle peer dependencies the same way as npm v7 [#4776](https://github.com/pnpm/pnpm/pull/4776).
- Updated dependencies [190f0b331]
  - @pnpm/core@5.2.0
  - @pnpm/cli-utils@0.7.7
  - @pnpm/find-workspace-packages@4.0.7
  - @pnpm/plugin-commands-rebuild@6.1.6
  - @pnpm/filter-workspace-packages@5.0.7

## 10.0.6

### Patch Changes

- Updated dependencies [af22c6c4f]
  - @pnpm/config@15.1.2
  - @pnpm/cli-utils@0.7.6
  - @pnpm/plugin-commands-rebuild@6.1.5
  - @pnpm/store-connection-manager@4.1.5
  - @pnpm/core@5.1.2
  - @pnpm/outdated@10.0.4
  - @pnpm/find-workspace-packages@4.0.6
  - @pnpm/package-store@13.0.5
  - @pnpm/filter-workspace-packages@5.0.6

## 10.0.5

### Patch Changes

- Updated dependencies [52b0576af]
  - @pnpm/cli-utils@0.7.5
  - @pnpm/find-workspace-packages@4.0.5
  - @pnpm/plugin-commands-rebuild@6.1.4
  - @pnpm/core@5.1.1
  - @pnpm/outdated@10.0.3
  - @pnpm/filter-workspace-packages@5.0.5
  - @pnpm/package-store@13.0.4
  - @pnpm/store-connection-manager@4.1.4

## 10.0.4

### Patch Changes

- 0075fcd23: `pnpm prune` works in a workspace [#4647](https://github.com/pnpm/pnpm/pull/4691).
- 0075fcd23: `pnpm prune` does not remove hoisted dependencies.
- 8ef4db94c: Export AddCommandOptions.
- Updated dependencies [cadefe5b6]
- Updated dependencies [315871260]
- Updated dependencies [0075fcd23]
  - @pnpm/core@5.1.0
  - @pnpm/package-store@13.0.3
  - @pnpm/cli-utils@0.7.4
  - @pnpm/store-connection-manager@4.1.3
  - @pnpm/find-workspace-packages@4.0.4
  - @pnpm/plugin-commands-rebuild@6.1.3
  - @pnpm/filter-workspace-packages@5.0.4
  - @pnpm/config@15.1.1

## 10.0.3

### Patch Changes

- af6ac00e4: Improve global linking.
- Updated dependencies [af6ac00e4]
- Updated dependencies [18ba5e2c0]
  - @pnpm/core@5.0.0
  - @pnpm/types@8.0.1
  - @pnpm/package-store@13.0.2
  - @pnpm/cli-utils@0.7.3
  - @pnpm/config@15.1.1
  - @pnpm/find-workspace-packages@4.0.3
  - @pnpm/manifest-utils@3.0.2
  - @pnpm/outdated@10.0.2
  - @pnpm/plugin-commands-rebuild@6.1.2
  - @pnpm/pnpmfile@2.0.2
  - @pnpm/read-project-manifest@3.0.2
  - @pnpm/resolver-base@9.0.1
  - @pnpm/sort-packages@3.0.2
  - @pnpm/store-connection-manager@4.1.2
  - @pnpm/filter-workspace-packages@5.0.3

## 10.0.2

### Patch Changes

- @pnpm/core@4.0.2

## 10.0.1

### Patch Changes

- 84c7e15ba: Exit with an error when running `pnpm install` in a directory that has no `package.json` file in it (and in parent directories) [#4609](https://github.com/pnpm/pnpm/issues/4609).
- c5caf8334: Allow to pass `global-bin-dir` through CLI options to the `add` command.
- Updated dependencies [9f0616282]
- Updated dependencies [c38feff08]
- Updated dependencies [e05dcc48a]
  - @pnpm/filter-workspace-packages@5.0.2
  - @pnpm/core@4.0.1
  - @pnpm/config@15.1.0
  - @pnpm/plugin-commands-rebuild@6.1.1
  - @pnpm/cli-utils@0.7.2
  - @pnpm/store-connection-manager@4.1.1
  - @pnpm/find-workspace-packages@4.0.2

## 10.0.0

### Major Changes

- e7bdc2cc2: Dependencies of the root workspace project are not used to resolve peer dependencies of other workspace projects [#4469](https://github.com/pnpm/pnpm/pull/4469).

### Patch Changes

- 01f4e9c91: `pnpm fetch` should allow the same CLI options as `pnpm install` [#4453](https://github.com/pnpm/pnpm/issues/4453).
- Updated dependencies [2109f2e8e]
- Updated dependencies [8fa95fd86]
- Updated dependencies [0a70aedb1]
- Updated dependencies [88289a42c]
- Updated dependencies [8dac029ef]
- Updated dependencies [e7bdc2cc2]
- Updated dependencies [aecd4acdd]
- Updated dependencies [72b79f55a]
- Updated dependencies [546e644e9]
- Updated dependencies [dbe366990]
- Updated dependencies [c6463b9fd]
- Updated dependencies [4bed585e2]
- Updated dependencies [8fa95fd86]
- Updated dependencies [618842b0d]
- Updated dependencies [1267e4eff]
  - @pnpm/core@4.0.0
  - @pnpm/plugin-commands-rebuild@6.1.0
  - @pnpm/sort-packages@3.0.1
  - @pnpm/config@15.0.0
  - @pnpm/store-connection-manager@4.1.0
  - @pnpm/manifest-utils@3.0.1
  - @pnpm/constants@6.1.0
  - @pnpm/outdated@10.0.1
  - @pnpm/package-store@13.0.1
  - @pnpm/cli-utils@0.7.1
  - @pnpm/error@3.0.1
  - @pnpm/find-workspace-packages@4.0.1
  - @pnpm/filter-workspace-packages@5.0.1
  - @pnpm/find-workspace-dir@4.0.1
  - @pnpm/pnpmfile@2.0.1
  - @pnpm/read-project-manifest@3.0.1

## 9.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- 9b9b13c3a: Update Yarn dependencies.
- Updated dependencies [516859178]
- Updated dependencies [d504dc380]
- Updated dependencies [a36b6026b]
- Updated dependencies [73d71a2d5]
- Updated dependencies [fa656992c]
- Updated dependencies [542014839]
- Updated dependencies [585e9ca9e]
  - @pnpm/config@14.0.0
  - @pnpm/core@3.0.0
  - @pnpm/types@8.0.0
  - @pnpm/command@3.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/error@3.0.0
  - @pnpm/filter-workspace-packages@5.0.0
  - @pnpm/find-workspace-dir@4.0.0
  - @pnpm/find-workspace-packages@4.0.0
  - @pnpm/graceful-fs@2.0.0
  - @pnpm/manifest-utils@3.0.0
  - @pnpm/matcher@3.0.0
  - @pnpm/outdated@10.0.0
  - @pnpm/package-store@13.0.0
  - @pnpm/parse-wanted-dependency@3.0.0
  - @pnpm/plugin-commands-rebuild@6.0.0
  - @pnpm/pnpmfile@2.0.0
  - @pnpm/read-project-manifest@3.0.0
  - @pnpm/resolver-base@9.0.0
  - @pnpm/sort-packages@3.0.0
  - @pnpm/store-connection-manager@4.0.0
  - @pnpm/cli-utils@0.7.0
  - @pnpm/common-cli-options-help@0.9.0

## 8.4.4

### Patch Changes

- @pnpm/core@2.7.3

## 8.4.3

### Patch Changes

- 2149d345a: pnpm i should work correctly on virtual drives instead of silently shutting down
- Updated dependencies [5c525db13]
- Updated dependencies [70ba51da9]
  - @pnpm/core@2.7.2
  - @pnpm/error@2.1.0
  - @pnpm/cli-utils@0.6.50
  - @pnpm/config@13.13.2
  - @pnpm/filter-workspace-packages@4.4.22
  - @pnpm/find-workspace-dir@3.0.2
  - @pnpm/manifest-utils@2.1.9
  - @pnpm/outdated@9.0.32
  - @pnpm/pnpmfile@1.2.6
  - @pnpm/read-project-manifest@2.0.13
  - @pnpm/store-connection-manager@3.2.10
  - @pnpm/package-store@12.1.12
  - @pnpm/plugin-commands-rebuild@5.4.14
  - @pnpm/find-workspace-packages@3.1.42

## 8.4.2

### Patch Changes

- 4e3b99ae0: `onlyBuiltDependencies` should work.
- Updated dependencies [4e3b99ae0]
  - @pnpm/core@2.7.1

## 8.4.1

### Patch Changes

- Updated dependencies [b138d048c]
- Updated dependencies [076c3753a]
- Updated dependencies [d84b73b15]
  - @pnpm/core@2.7.0
  - @pnpm/types@7.10.0
  - @pnpm/package-store@12.1.11
  - @pnpm/outdated@9.0.31
  - @pnpm/pnpmfile@1.2.5
  - @pnpm/cli-utils@0.6.49
  - @pnpm/config@13.13.1
  - @pnpm/find-workspace-packages@3.1.41
  - @pnpm/manifest-utils@2.1.8
  - @pnpm/plugin-commands-rebuild@5.4.13
  - @pnpm/read-project-manifest@2.0.12
  - @pnpm/resolver-base@8.1.6
  - @pnpm/sort-packages@2.1.8
  - @pnpm/store-connection-manager@3.2.9
  - @pnpm/filter-workspace-packages@4.4.21

## 8.4.0

### Minor Changes

- e96fe4d9e: `pnpm uni` is an alias of `pnpm uninstall`.

## 8.3.9

### Patch Changes

- Updated dependencies [fa4f9133b]
  - @pnpm/package-store@12.1.10
  - @pnpm/core@2.6.0
  - @pnpm/store-connection-manager@3.2.8
  - @pnpm/plugin-commands-rebuild@5.4.12

## 8.3.8

### Patch Changes

- Updated dependencies [50e347d23]
  - @pnpm/package-store@12.1.9
  - @pnpm/core@2.6.0
  - @pnpm/store-connection-manager@3.2.7
  - @pnpm/plugin-commands-rebuild@5.4.11

## 8.3.7

### Patch Changes

- Updated dependencies [329e186e9]
  - @pnpm/core@2.6.0
  - @pnpm/plugin-commands-rebuild@5.4.10

## 8.3.6

### Patch Changes

- Updated dependencies [cc727797f]
  - @pnpm/core@2.5.4

## 8.3.5

### Patch Changes

- Updated dependencies [334e5340a]
- Updated dependencies [37d09a68f]
  - @pnpm/config@13.13.0
  - @pnpm/core@2.5.3
  - @pnpm/cli-utils@0.6.48
  - @pnpm/plugin-commands-rebuild@5.4.9
  - @pnpm/store-connection-manager@3.2.6
  - @pnpm/find-workspace-packages@3.1.40
  - @pnpm/filter-workspace-packages@4.4.20

## 8.3.4

### Patch Changes

- Updated dependencies [c1383044d]
  - @pnpm/core@2.5.2

## 8.3.3

### Patch Changes

- Updated dependencies [b7566b979]
  - @pnpm/config@13.12.0
  - @pnpm/cli-utils@0.6.47
  - @pnpm/plugin-commands-rebuild@5.4.8
  - @pnpm/store-connection-manager@3.2.5
  - @pnpm/find-workspace-packages@3.1.39
  - @pnpm/core@2.5.1
  - @pnpm/filter-workspace-packages@4.4.19

## 8.3.2

### Patch Changes

- Updated dependencies [cdc521cfa]
  - @pnpm/core@2.5.0
  - @pnpm/outdated@9.0.30
  - @pnpm/plugin-commands-rebuild@5.4.7
  - @pnpm/config@13.11.0
  - @pnpm/package-store@12.1.8

## 8.3.1

### Patch Changes

- Updated dependencies [08d781b80]
  - @pnpm/core@2.4.1

## 8.3.0

### Minor Changes

- 1cadc231a: Side effects cache is not an experimental feature anymore.

  Side effects cache is saved separately for packages with different dependencies. So if `foo` has `bar` in the dependencies, then a separate cache will be created each time `foo` is installed with a different version of `bar` [#4238](https://github.com/pnpm/pnpm/pull/4238).

### Patch Changes

- Updated dependencies [fff0e4493]
- Updated dependencies [1cadc231a]
- Updated dependencies [8a2cad034]
- Updated dependencies [4bdf7bcac]
  - @pnpm/config@13.11.0
  - @pnpm/core@2.4.0
  - @pnpm/manifest-utils@2.1.7
  - @pnpm/cli-utils@0.6.46
  - @pnpm/plugin-commands-rebuild@5.4.6
  - @pnpm/store-connection-manager@3.2.4
  - @pnpm/outdated@9.0.29
  - @pnpm/find-workspace-packages@3.1.38
  - @pnpm/filter-workspace-packages@4.4.18

## 8.2.1

### Patch Changes

- @pnpm/cli-utils@0.6.45
- @pnpm/find-workspace-packages@3.1.37
- @pnpm/plugin-commands-rebuild@5.4.5
- @pnpm/filter-workspace-packages@4.4.17

## 8.2.0

### Minor Changes

- 26cd01b88: In order to mute some types of peer dependency warnings, a new section in `package.json` may be used for declaring peer dependency warning rules. For example, the next configuration will turn off any warnings about missing `babel-loader` peer dependency and about `@angular/common`, when the wanted version of `@angular/common` is not v13.

  ```json
  {
    "name": "foo",
    "version": "0.0.0",
    "pnpm": {
      "peerDependencyRules": {
        "ignoreMissing": ["babel-loader"],
        "allowedVersions": {
          "@angular/common": "13"
        }
      }
    }
  }
  ```

- e76151f66: New setting supported: `auto-install-peers`. When it is set to `true`, `pnpm add <pkg>` automatically installs any missing peer dependencies as `devDependencies`.

### Patch Changes

- Updated dependencies [50ee25ae2]
- Updated dependencies [26cd01b88]
- Updated dependencies [e76151f66]
- Updated dependencies [26cd01b88]
- Updated dependencies [e76151f66]
  - @pnpm/core@2.3.0
  - @pnpm/config@13.10.0
  - @pnpm/types@7.9.0
  - @pnpm/cli-utils@0.6.44
  - @pnpm/plugin-commands-rebuild@5.4.4
  - @pnpm/store-connection-manager@3.2.3
  - @pnpm/find-workspace-packages@3.1.36
  - @pnpm/manifest-utils@2.1.6
  - @pnpm/outdated@9.0.28
  - @pnpm/package-store@12.1.8
  - @pnpm/pnpmfile@1.2.4
  - @pnpm/read-project-manifest@2.0.11
  - @pnpm/resolver-base@8.1.5
  - @pnpm/sort-packages@2.1.7
  - @pnpm/filter-workspace-packages@4.4.16

## 8.1.6

### Patch Changes

- ea24c69fe: `@pnpm/matcher` should be a prod dependency.
  - @pnpm/core@2.2.6
  - @pnpm/cli-utils@0.6.43
  - @pnpm/find-workspace-packages@3.1.35
  - @pnpm/plugin-commands-rebuild@5.4.3
  - @pnpm/filter-workspace-packages@4.4.15

## 8.1.5

### Patch Changes

- cbd2f3e2a: Downgrade and pin Yarn lib versions.
  - @pnpm/core@2.2.5

## 8.1.4

### Patch Changes

- @pnpm/core@2.2.4
- @pnpm/package-store@12.1.7
- @pnpm/store-connection-manager@3.2.2
- @pnpm/plugin-commands-rebuild@5.4.2

## 8.1.3

### Patch Changes

- @pnpm/core@2.2.3

## 8.1.2

### Patch Changes

- 0da31c70c: Allow to set `node-linker` through CLI options.
- Updated dependencies [7bac7e8be]
- Updated dependencies [8fe8f5e55]
- Updated dependencies [7375396db]
  - @pnpm/core@2.2.2
  - @pnpm/config@13.9.0
  - @pnpm/cli-utils@0.6.42
  - @pnpm/plugin-commands-rebuild@5.4.1
  - @pnpm/store-connection-manager@3.2.1
  - @pnpm/outdated@9.0.27
  - @pnpm/find-workspace-packages@3.1.34
  - @pnpm/filter-workspace-packages@4.4.14
  - @pnpm/package-store@12.1.6

## 8.1.1

### Patch Changes

- @pnpm/core@2.2.1

## 8.1.0

### Minor Changes

- a6cf11cb7: New optional setting added: userConfig. userConfig may contain token helpers.

### Patch Changes

- Updated dependencies [a6cf11cb7]
- Updated dependencies [732d4962f]
- Updated dependencies [a6cf11cb7]
  - @pnpm/plugin-commands-rebuild@5.4.0
  - @pnpm/store-connection-manager@3.2.0
  - @pnpm/config@13.8.0
  - @pnpm/core@2.2.0
  - @pnpm/outdated@9.0.26
  - @pnpm/package-store@12.1.6
  - @pnpm/cli-utils@0.6.41
  - @pnpm/find-workspace-packages@3.1.33
  - @pnpm/filter-workspace-packages@4.4.13

## 8.0.7

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/core@2.1.4
  - @pnpm/plugin-commands-rebuild@5.3.10
  - @pnpm/cli-utils@0.6.40
  - @pnpm/config@13.7.2
  - @pnpm/find-workspace-packages@3.1.32
  - @pnpm/manifest-utils@2.1.5
  - @pnpm/outdated@9.0.25
  - @pnpm/package-store@12.1.6
  - @pnpm/pnpmfile@1.2.3
  - @pnpm/read-project-manifest@2.0.10
  - @pnpm/resolver-base@8.1.4
  - @pnpm/sort-packages@2.1.6
  - @pnpm/store-connection-manager@3.1.17
  - @pnpm/filter-workspace-packages@4.4.12

## 8.0.6

### Patch Changes

- 08380076f: Document `--no-frozen-lockfile` explicitly in the help of the install command.
- Updated dependencies [08380076f]
  - @pnpm/core@2.1.3
  - @pnpm/outdated@9.0.24
  - @pnpm/plugin-commands-rebuild@5.3.9

## 8.0.5

### Patch Changes

- 40dc2f954: `pnpm update pkg` should not fail if `pkg` not found as a direct dependency, unless `--depth=0` is passed as a CLI option [#4122](https://github.com/pnpm/pnpm/issues/4122).
- Updated dependencies [cb2e4e33a]
  - @pnpm/core@2.1.2
  - @pnpm/cli-utils@0.6.39
  - @pnpm/find-workspace-packages@3.1.31
  - @pnpm/plugin-commands-rebuild@5.3.8
  - @pnpm/filter-workspace-packages@4.4.11

## 8.0.4

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/cli-utils@0.6.38
  - @pnpm/config@13.7.1
  - @pnpm/core@2.1.1
  - @pnpm/find-workspace-packages@3.1.30
  - @pnpm/manifest-utils@2.1.4
  - @pnpm/outdated@9.0.23
  - @pnpm/package-store@12.1.5
  - @pnpm/plugin-commands-rebuild@5.3.7
  - @pnpm/pnpmfile@1.2.2
  - @pnpm/read-project-manifest@2.0.9
  - @pnpm/resolver-base@8.1.3
  - @pnpm/sort-packages@2.1.5
  - @pnpm/store-connection-manager@3.1.16
  - @pnpm/filter-workspace-packages@4.4.10

## 8.0.3

### Patch Changes

- Updated dependencies [30bfca967]
- Updated dependencies [5af305f39]
- Updated dependencies [927c4a089]
- Updated dependencies [10a4bd4db]
- Updated dependencies [ba9b2eba1]
- Updated dependencies [d00e1fc6a]
- Updated dependencies [25f0fa9fa]
  - @pnpm/config@13.7.0
  - @pnpm/core@2.1.0
  - @pnpm/common-cli-options-help@0.8.0
  - @pnpm/types@7.7.0
  - @pnpm/package-store@12.1.4
  - @pnpm/manifest-utils@2.1.3
  - @pnpm/plugin-commands-rebuild@5.3.6
  - @pnpm/pnpmfile@1.2.1
  - @pnpm/cli-utils@0.6.37
  - @pnpm/filter-workspace-packages@4.4.9
  - @pnpm/store-connection-manager@3.1.15
  - @pnpm/find-workspace-packages@3.1.29
  - @pnpm/outdated@9.0.22
  - @pnpm/read-project-manifest@2.0.8
  - @pnpm/resolver-base@8.1.2
  - @pnpm/sort-packages@2.1.4

## 8.0.2

### Patch Changes

- @pnpm/core@2.0.1
- @pnpm/outdated@9.0.21
- @pnpm/package-store@12.1.3
- @pnpm/store-connection-manager@3.1.14
- @pnpm/plugin-commands-rebuild@5.3.5

## 8.0.1

### Patch Changes

- Updated dependencies [46aaf7108]
  - @pnpm/config@13.6.1
  - @pnpm/cli-utils@0.6.36
  - @pnpm/plugin-commands-rebuild@5.3.4
  - @pnpm/store-connection-manager@3.1.13
  - @pnpm/core@2.0.1
  - @pnpm/outdated@9.0.20
  - @pnpm/package-store@12.1.3
  - @pnpm/find-workspace-packages@3.1.28
  - @pnpm/filter-workspace-packages@4.4.8

## 8.0.0

### Major Changes

- 8a99a01ff: Pass `packageExtensions`, `overrides`, and `neverBuiltDependencies` to the core API. Take this information from `rootProjectManifest`, which should be passed in via the options.

### Patch Changes

- Updated dependencies [8a99a01ff]
- Updated dependencies [8a99a01ff]
  - @pnpm/core@2.0.0
  - @pnpm/config@13.6.0
  - @pnpm/outdated@9.0.19
  - @pnpm/plugin-commands-rebuild@5.3.3
  - @pnpm/cli-utils@0.6.35
  - @pnpm/store-connection-manager@3.1.12
  - @pnpm/find-workspace-packages@3.1.27
  - @pnpm/filter-workspace-packages@4.4.7

## 7.4.0

### Minor Changes

- 3b4dc17c7: feat: support yarn v2 lockfile transform

### Patch Changes

- @pnpm/cli-utils@0.6.34
- @pnpm/core@1.3.2
- @pnpm/outdated@9.0.18
- @pnpm/package-store@12.1.3
- @pnpm/store-connection-manager@3.1.11
- @pnpm/find-workspace-packages@3.1.26
- @pnpm/plugin-commands-rebuild@5.3.2
- @pnpm/filter-workspace-packages@4.4.6

## 7.3.2

### Patch Changes

- Updated dependencies [a7ff2d5ce]
  - @pnpm/config@13.5.1
  - @pnpm/cli-utils@0.6.33
  - @pnpm/plugin-commands-rebuild@5.3.1
  - @pnpm/store-connection-manager@3.1.10
  - @pnpm/core@1.3.2
  - @pnpm/package-store@12.1.3
  - @pnpm/find-workspace-packages@3.1.25
  - @pnpm/filter-workspace-packages@4.4.5

## 7.3.1

### Patch Changes

- Updated dependencies [fe9818220]
  - @pnpm/core@1.3.1

## 7.3.0

### Minor Changes

- 002778559: New setting added: `scriptsPrependNodePath`. This setting can be `true`, `false`, or `warn-only`.
  When `true`, the path to the `node` executable with which pnpm executed is prepended to the `PATH` of the scripts.
  When `warn-only`, pnpm will print a warning if the scripts run with a `node` binary that differs from the `node` binary executing the pnpm CLI.

### Patch Changes

- Updated dependencies [002778559]
  - @pnpm/config@13.5.0
  - @pnpm/core@1.3.0
  - @pnpm/plugin-commands-rebuild@5.3.0
  - @pnpm/cli-utils@0.6.32
  - @pnpm/store-connection-manager@3.1.9
  - @pnpm/find-workspace-packages@3.1.24
  - @pnpm/outdated@9.0.17
  - @pnpm/package-store@12.1.2
  - @pnpm/filter-workspace-packages@4.4.4

## 7.2.6

### Patch Changes

- @pnpm/core@1.2.3
- @pnpm/outdated@9.0.16
- @pnpm/package-store@12.1.2
- @pnpm/store-connection-manager@3.1.8
- @pnpm/plugin-commands-rebuild@5.2.6

## 7.2.5

### Patch Changes

- @pnpm/core@1.2.2
- @pnpm/outdated@9.0.15
- @pnpm/package-store@12.1.2
- @pnpm/store-connection-manager@3.1.7
- @pnpm/plugin-commands-rebuild@5.2.5

## 7.2.4

### Patch Changes

- Updated dependencies [1647d8e2f]
- Updated dependencies [bb0f8bc16]
  - @pnpm/store-connection-manager@3.1.6
  - @pnpm/core@1.2.1
  - @pnpm/plugin-commands-rebuild@5.2.4
  - @pnpm/cli-utils@0.6.31
  - @pnpm/find-workspace-packages@3.1.23
  - @pnpm/package-store@12.1.2
  - @pnpm/filter-workspace-packages@4.4.3

## 7.2.3

### Patch Changes

- Updated dependencies [302ae4f6f]
- Updated dependencies [b75993dde]
- Updated dependencies [2511c82cd]
  - @pnpm/core@1.2.0
  - @pnpm/pnpmfile@1.2.0
  - @pnpm/types@7.6.0
  - @pnpm/plugin-commands-rebuild@5.2.3
  - @pnpm/config@13.4.2
  - @pnpm/cli-utils@0.6.30
  - @pnpm/find-workspace-packages@3.1.22
  - @pnpm/manifest-utils@2.1.2
  - @pnpm/outdated@9.0.14
  - @pnpm/package-store@12.1.1
  - @pnpm/read-project-manifest@2.0.7
  - @pnpm/resolver-base@8.1.1
  - @pnpm/sort-packages@2.1.3
  - @pnpm/store-connection-manager@3.1.5
  - @pnpm/filter-workspace-packages@4.4.2

## 7.2.2

### Patch Changes

- @pnpm/core@1.1.2
- @pnpm/plugin-commands-rebuild@5.2.2

## 7.2.1

### Patch Changes

- @pnpm/core@1.1.1

## 7.2.0

### Minor Changes

- 6a54be5a1: Support pnpm recursive import

### Patch Changes

- @pnpm/core@1.1.0
- @pnpm/outdated@9.0.13
- @pnpm/package-store@12.1.0
- @pnpm/store-connection-manager@3.1.4
- @pnpm/plugin-commands-rebuild@5.2.1

## 7.1.0

### Minor Changes

- 4ab87844a: New property supported via the `dependenciesMeta` field of `package.json`: `injected`. When `injected` is set to `true`, the package will be hard linked to `node_modules`, not symlinked [#3915](https://github.com/pnpm/pnpm/pull/3915).

  For instance, the following `package.json` in a workspace will create a symlink to `bar` in the `node_modules` directory of `foo`:

  ```json
  {
    "name": "foo",
    "dependencies": {
      "bar": "workspace:1.0.0"
    }
  }
  ```

  But what if `bar` has `react` in its peer dependencies? If all projects in the monorepo use the same version of `react`, then no problem. But what if `bar` is required by `foo` that uses `react` 16 and `qar` with `react` 17? In the past, you'd have to choose a single version of react and install it as dev dependency of `bar`. But now with the `injected` field you can inject `bar` to a package, and `bar` will be installed with the `react` version of that package.

  So this will be the `package.json` of `foo`:

  ```json
  {
    "name": "foo",
    "dependencies": {
      "bar": "workspace:1.0.0",
      "react": "16"
    },
    "dependenciesMeta": {
      "bar": {
        "injected": true
      }
    }
  }
  ```

  `bar` will be hard linked into the dependencies of `foo`, and `react` 16 will be linked to the dependencies of `foo/node_modules/bar`.

  And this will be the `package.json` of `qar`:

  ```json
  {
    "name": "qar",
    "dependencies": {
      "bar": "workspace:1.0.0",
      "react": "17"
    },
    "dependenciesMeta": {
      "bar": {
        "injected": true
      }
    }
  }
  ```

  `bar` will be hard linked into the dependencies of `qar`, and `react` 17 will be linked to the dependencies of `qar/node_modules/bar`.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/resolver-base@8.1.0
  - @pnpm/package-store@12.1.0
  - @pnpm/plugin-commands-rebuild@5.2.0
  - @pnpm/core@1.1.0
  - @pnpm/cli-utils@0.6.29
  - @pnpm/config@13.4.1
  - @pnpm/find-workspace-packages@3.1.21
  - @pnpm/manifest-utils@2.1.1
  - @pnpm/outdated@9.0.12
  - @pnpm/pnpmfile@1.1.1
  - @pnpm/sort-packages@2.1.2
  - @pnpm/store-connection-manager@3.1.3
  - @pnpm/filter-workspace-packages@4.4.1

## 7.0.5

### Patch Changes

- @pnpm/core@1.0.2
- @pnpm/plugin-commands-rebuild@5.1.5
- @pnpm/outdated@9.0.11
- @pnpm/package-store@12.0.15
- @pnpm/store-connection-manager@3.1.2

## 7.0.4

### Patch Changes

- 2ba869c25: Do not explicitly link to Node.js executables that are not in the pnpm home directory.
- Updated dependencies [b6d74c545]
- Updated dependencies [456232654]
  - @pnpm/config@13.4.0
  - @pnpm/filter-workspace-packages@4.4.0
  - @pnpm/cli-utils@0.6.28
  - @pnpm/plugin-commands-rebuild@5.1.4
  - @pnpm/store-connection-manager@3.1.1
  - @pnpm/find-workspace-packages@3.1.20
  - @pnpm/core@1.0.1
  - @pnpm/outdated@9.0.10
  - @pnpm/package-store@12.0.15

## 7.0.3

### Patch Changes

- Updated dependencies [bd7bcdbe8]
- Updated dependencies [d4e2e52c4]
- Updated dependencies [bd7bcdbe8]
  - @pnpm/store-connection-manager@3.1.0
  - @pnpm/core@1.0.0
  - @pnpm/config@13.3.0
  - @pnpm/plugin-commands-rebuild@5.1.3
  - @pnpm/cli-utils@0.6.27
  - @pnpm/find-workspace-packages@3.1.19
  - @pnpm/outdated@9.0.9
  - @pnpm/package-store@12.0.15
  - @pnpm/filter-workspace-packages@4.3.3

## 7.0.2

### Patch Changes

- f10a53ee7: Add `--save-prefix` and `--cache-dir` to the list of allowed CLI options.
- 7b0906043: add help for `--force` option of `install` command (re: #3878)
- Updated dependencies [5ee3b2dc7]
  - @pnpm/config@13.2.0
  - @pnpm/cli-utils@0.6.26
  - @pnpm/plugin-commands-rebuild@5.1.2
  - @pnpm/store-connection-manager@3.0.20
  - supi@0.47.27
  - @pnpm/find-workspace-packages@3.1.18
  - @pnpm/filter-workspace-packages@4.3.2

## 7.0.1

### Patch Changes

- cd980742c: Always prefer the system-default Node.js executable when pinning a globally installed package to a Node.js version.
  - @pnpm/cli-utils@0.6.25
  - @pnpm/find-workspace-packages@3.1.17
  - @pnpm/plugin-commands-rebuild@5.1.1
  - @pnpm/filter-workspace-packages@4.3.1

## 7.0.0

### Major Changes

- 4027a3c69: Hooks are passed in to the installation plugin, except when recursive installation happens on projects with own `.pnpmfile.js` files.

### Patch Changes

- Updated dependencies [ef9d2719a]
- Updated dependencies [4027a3c69]
- Updated dependencies [4027a3c69]
- Updated dependencies [1efaaf706]
- Updated dependencies [4027a3c69]
  - @pnpm/pnpmfile@1.1.0
  - @pnpm/plugin-commands-rebuild@5.1.0
  - @pnpm/config@13.1.0
  - @pnpm/common-cli-options-help@0.7.1
  - @pnpm/cli-utils@0.6.24
  - @pnpm/store-connection-manager@3.0.19
  - @pnpm/find-workspace-packages@3.1.16
  - supi@0.47.25
  - @pnpm/filter-workspace-packages@4.2.1

## 6.2.2

### Patch Changes

- Updated dependencies [59a4152ce]
  - supi@0.47.24
  - @pnpm/plugin-commands-rebuild@5.0.19
  - @pnpm/outdated@9.0.8
  - @pnpm/package-store@12.0.15
  - @pnpm/store-connection-manager@3.0.18

## 6.2.1

### Patch Changes

- Updated dependencies [fe5688dc0]
- Updated dependencies [c7081cbb4]
- Updated dependencies [c7081cbb4]
  - @pnpm/common-cli-options-help@0.7.0
  - @pnpm/config@13.0.0
  - @pnpm/filter-workspace-packages@4.2.0
  - supi@0.47.23
  - @pnpm/plugin-commands-rebuild@5.0.18
  - @pnpm/cli-utils@0.6.23
  - @pnpm/store-connection-manager@3.0.17
  - @pnpm/find-workspace-packages@3.1.15

## 6.2.0

### Minor Changes

- 553a5d840: Globally installed packages should always use the active version of Node.js. So if webpack is installed while Node.js 16 is active, webpack will be executed using Node.js 16 even if the active Node.js version is switched using `pnpm env`.

### Patch Changes

- Updated dependencies [83e23601e]
- Updated dependencies [6cc1aa2c0]
- Updated dependencies [553a5d840]
- Updated dependencies [d62259d67]
  - supi@0.47.22
  - @pnpm/manifest-utils@2.1.0
  - @pnpm/config@12.6.0
  - @pnpm/cli-utils@0.6.22
  - @pnpm/outdated@9.0.7
  - @pnpm/plugin-commands-rebuild@5.0.17
  - @pnpm/store-connection-manager@3.0.16
  - @pnpm/find-workspace-packages@3.1.14
  - @pnpm/filter-workspace-packages@4.1.17

## 6.1.1

### Patch Changes

- Updated dependencies [141d2f02e]
- Updated dependencies [04b7f6086]
  - supi@0.47.21
  - @pnpm/filter-workspace-packages@4.1.16
  - @pnpm/plugin-commands-rebuild@5.0.16
  - @pnpm/outdated@9.0.6
  - @pnpm/package-store@12.0.15
  - @pnpm/store-connection-manager@3.0.15

## 6.1.0

### Minor Changes

- 11a934da1: Adding --fix-lockfile for the install command to support autofix broken lockfile

### Patch Changes

- Updated dependencies [6681fdcbc]
- Updated dependencies [11a934da1]
  - @pnpm/config@12.5.0
  - supi@0.47.20
  - @pnpm/cli-utils@0.6.21
  - @pnpm/plugin-commands-rebuild@5.0.15
  - @pnpm/store-connection-manager@3.0.14
  - @pnpm/package-store@12.0.15
  - @pnpm/find-workspace-packages@3.1.13
  - @pnpm/outdated@9.0.5
  - @pnpm/filter-workspace-packages@4.1.15

## 6.0.19

### Patch Changes

- @pnpm/cli-utils@0.6.20
- @pnpm/outdated@9.0.4
- @pnpm/package-store@12.0.14
- @pnpm/store-connection-manager@3.0.13
- supi@0.47.19
- @pnpm/plugin-commands-rebuild@5.0.14
- @pnpm/find-workspace-packages@3.1.12
- @pnpm/filter-workspace-packages@4.1.14

## 6.0.18

### Patch Changes

- Updated dependencies [ccf2f295d]
  - supi@0.47.18

## 6.0.17

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9
  - @pnpm/cli-utils@0.6.19
  - @pnpm/plugin-commands-rebuild@5.0.13
  - @pnpm/store-connection-manager@3.0.12
  - @pnpm/find-workspace-packages@3.1.11
  - supi@0.47.17
  - @pnpm/filter-workspace-packages@4.1.13

## 6.0.16

### Patch Changes

- @pnpm/config@12.4.8
- @pnpm/cli-utils@0.6.18
- @pnpm/plugin-commands-rebuild@5.0.12
- @pnpm/store-connection-manager@3.0.11
- @pnpm/find-workspace-packages@3.1.10
- supi@0.47.16
- @pnpm/filter-workspace-packages@4.1.12

## 6.0.15

### Patch Changes

- supi@0.47.15
- @pnpm/outdated@9.0.3
- @pnpm/package-store@12.0.14
- @pnpm/store-connection-manager@3.0.10
- @pnpm/plugin-commands-rebuild@5.0.11

## 6.0.14

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7
  - @pnpm/cli-utils@0.6.17
  - @pnpm/plugin-commands-rebuild@5.0.10
  - @pnpm/store-connection-manager@3.0.9
  - @pnpm/package-store@12.0.14
  - supi@0.47.14
  - @pnpm/find-workspace-packages@3.1.9
  - @pnpm/filter-workspace-packages@4.1.11

## 6.0.13

### Patch Changes

- supi@0.47.13
- @pnpm/package-store@12.0.13
- @pnpm/store-connection-manager@3.0.8
- @pnpm/plugin-commands-rebuild@5.0.9

## 6.0.12

### Patch Changes

- c3d2746ac: Peer dependencies are resolved from the root of the workspace when a new dependency is added to the root of the workspace.
  - supi@0.47.12
  - @pnpm/outdated@9.0.2
  - @pnpm/package-store@12.0.12
  - @pnpm/store-connection-manager@3.0.7
  - @pnpm/plugin-commands-rebuild@5.0.8

## 6.0.11

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6
  - @pnpm/cli-utils@0.6.16
  - @pnpm/plugin-commands-rebuild@5.0.7
  - @pnpm/store-connection-manager@3.0.6
  - @pnpm/find-workspace-packages@3.1.8
  - supi@0.47.11
  - @pnpm/filter-workspace-packages@4.1.10

## 6.0.10

### Patch Changes

- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5
  - supi@0.47.10
  - @pnpm/cli-utils@0.6.15
  - @pnpm/plugin-commands-rebuild@5.0.6
  - @pnpm/store-connection-manager@3.0.5
  - @pnpm/find-workspace-packages@3.1.7
  - @pnpm/filter-workspace-packages@4.1.9

## 6.0.9

### Patch Changes

- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4
  - supi@0.47.9
  - @pnpm/cli-utils@0.6.14
  - @pnpm/plugin-commands-rebuild@5.0.5
  - @pnpm/store-connection-manager@3.0.4
  - @pnpm/find-workspace-packages@3.1.6
  - @pnpm/filter-workspace-packages@4.1.8

## 6.0.8

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/cli-utils@0.6.13
  - @pnpm/config@12.4.3
  - @pnpm/find-workspace-packages@3.1.5
  - @pnpm/manifest-utils@2.0.4
  - @pnpm/outdated@9.0.1
  - @pnpm/package-store@12.0.12
  - @pnpm/plugin-commands-rebuild@5.0.4
  - @pnpm/pnpmfile@1.0.5
  - @pnpm/resolver-base@8.0.4
  - @pnpm/sort-packages@2.1.1
  - supi@0.47.8
  - @pnpm/store-connection-manager@3.0.3
  - @pnpm/filter-workspace-packages@4.1.7

## 6.0.7

### Patch Changes

- Updated dependencies [73c1f802e]
  - @pnpm/config@12.4.2
  - @pnpm/plugin-commands-rebuild@5.0.3
  - supi@0.47.7
  - @pnpm/cli-utils@0.6.12
  - @pnpm/store-connection-manager@3.0.2
  - @pnpm/find-workspace-packages@3.1.4
  - @pnpm/filter-workspace-packages@4.1.6

## 6.0.6

### Patch Changes

- Updated dependencies [3c044519e]
  - supi@0.47.6

## 6.0.5

### Patch Changes

- Updated dependencies [040124530]
  - supi@0.47.5
  - @pnpm/cli-utils@0.6.11
  - @pnpm/find-workspace-packages@3.1.3
  - @pnpm/plugin-commands-rebuild@5.0.2
  - @pnpm/filter-workspace-packages@4.1.5

## 6.0.4

### Patch Changes

- Updated dependencies [ca67f6004]
  - supi@0.47.4

## 6.0.3

### Patch Changes

- Updated dependencies [caf453dd3]
  - supi@0.47.3

## 6.0.2

### Patch Changes

- d3ec941d2: `pnpm link -g <pkg>` should not break node_modules of the target project.
- d3ec941d2: Do not run installation in the global package, when linking a dependency using `pnpm link -g <pkg name>`.
- Updated dependencies [d3ec941d2]
  - supi@0.47.2

## 6.0.1

### Patch Changes

- 8678e2553: An error should be thrown if `pnpm link` is executed with no parameters and no options.
- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1
  - @pnpm/cli-utils@0.6.10
  - @pnpm/plugin-commands-rebuild@5.0.1
  - @pnpm/store-connection-manager@3.0.1
  - @pnpm/find-workspace-packages@3.1.2
  - supi@0.47.1
  - @pnpm/filter-workspace-packages@4.1.4

## 6.0.0

### Major Changes

- 691f64713: New required option added: cacheDir.

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [691f64713]
- Updated dependencies [b3478c756]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0
  - @pnpm/outdated@9.0.0
  - @pnpm/plugin-commands-rebuild@5.0.0
  - @pnpm/store-connection-manager@3.0.0
  - supi@0.47.0
  - @pnpm/cli-utils@0.6.9
  - @pnpm/package-store@12.0.11
  - @pnpm/find-workspace-packages@3.1.1
  - @pnpm/filter-workspace-packages@4.1.3

## 5.2.0

### Minor Changes

- 5565dd5f4: Use a more detailed cyclic dependencies warning

### Patch Changes

- Updated dependencies [a5bde0aa2]
  - @pnpm/find-workspace-packages@3.1.0
  - @pnpm/filter-workspace-packages@4.1.2
  - @pnpm/plugin-commands-rebuild@4.0.12

## 5.1.1

### Patch Changes

- supi@0.46.18

## 5.1.0

### Minor Changes

- 1442f8786: Warn about cyclic dependencies on install

### Patch Changes

- Updated dependencies [1442f8786]
- Updated dependencies [8e76690f4]
- Updated dependencies [8e76690f4]
  - @pnpm/sort-packages@2.1.0
  - supi@0.46.17
  - @pnpm/types@7.3.0
  - @pnpm/outdated@8.0.14
  - @pnpm/plugin-commands-rebuild@4.0.11
  - @pnpm/cli-utils@0.6.8
  - @pnpm/config@12.3.3
  - @pnpm/find-workspace-packages@3.0.8
  - @pnpm/manifest-utils@2.0.3
  - @pnpm/package-store@12.0.11
  - @pnpm/pnpmfile@1.0.4
  - @pnpm/resolver-base@8.0.3
  - @pnpm/store-connection-manager@2.1.11
  - @pnpm/filter-workspace-packages@4.1.1

## 5.0.0

### Major Changes

- 5950459d7: The update command reads the production/dev/optional options from the cliOptions. So when the settings are set via the config file, they are ignored by the update command.

### Patch Changes

- 72c0dd7be: The remove command should read the production/optional/dev options.
- Updated dependencies [c86fad004]
  - @pnpm/filter-workspace-packages@4.1.0
  - @pnpm/outdated@8.0.13
  - @pnpm/plugin-commands-rebuild@4.0.10
  - supi@0.46.16
  - @pnpm/package-store@12.0.10
  - @pnpm/store-connection-manager@2.1.10

## 4.1.13

### Patch Changes

- @pnpm/outdated@8.0.12
- @pnpm/package-store@12.0.9
- @pnpm/store-connection-manager@2.1.9
- supi@0.46.15
- @pnpm/plugin-commands-rebuild@4.0.9

## 4.1.12

### Patch Changes

- @pnpm/outdated@8.0.11
- supi@0.46.15
- @pnpm/plugin-commands-rebuild@4.0.8

## 4.1.11

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/outdated@8.0.10
  - @pnpm/package-store@12.0.9
  - @pnpm/store-connection-manager@2.1.8
  - supi@0.46.14
  - @pnpm/cli-utils@0.6.7
  - @pnpm/config@12.3.2
  - @pnpm/find-workspace-packages@3.0.7
  - @pnpm/manifest-utils@2.0.2
  - @pnpm/plugin-commands-rebuild@4.0.7
  - @pnpm/pnpmfile@1.0.3
  - @pnpm/resolver-base@8.0.2
  - @pnpm/sort-packages@2.0.2
  - @pnpm/filter-workspace-packages@4.0.6

## 4.1.10

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1
  - @pnpm/filter-workspace-packages@4.0.5
  - @pnpm/outdated@8.0.9
  - @pnpm/plugin-commands-rebuild@4.0.6
  - supi@0.46.13
  - @pnpm/cli-utils@0.6.6
  - @pnpm/store-connection-manager@2.1.7
  - @pnpm/package-store@12.0.8
  - @pnpm/find-workspace-packages@3.0.6

## 4.1.9

### Patch Changes

- @pnpm/outdated@8.0.8
- @pnpm/package-store@12.0.7
- @pnpm/store-connection-manager@2.1.6
- supi@0.46.12
- @pnpm/plugin-commands-rebuild@4.0.5

## 4.1.8

### Patch Changes

- Updated dependencies [84ec82e05]
- Updated dependencies [6e8cedb79]
- Updated dependencies [c2a71e4fd]
- Updated dependencies [84ec82e05]
  - @pnpm/config@12.3.0
  - @pnpm/find-workspace-dir@3.0.1
  - @pnpm/common-cli-options-help@0.6.0
  - @pnpm/cli-utils@0.6.5
  - @pnpm/plugin-commands-rebuild@4.0.4
  - @pnpm/store-connection-manager@2.1.5
  - @pnpm/find-workspace-packages@3.0.5
  - supi@0.46.12
  - @pnpm/filter-workspace-packages@4.0.4

## 4.1.7

### Patch Changes

- Updated dependencies [da0d4091d]
  - @pnpm/pnpmfile@1.0.2
  - supi@0.46.11

## 4.1.6

### Patch Changes

- 9d2ff0309: Fix: align pnpm save-prefix behavior when a range is not specified explicitly.
- Updated dependencies [0e69ad440]
  - supi@0.46.10

## 4.1.5

### Patch Changes

- @pnpm/outdated@8.0.7
- @pnpm/plugin-commands-rebuild@4.0.3
- supi@0.46.9
- @pnpm/cli-utils@0.6.4
- @pnpm/package-store@12.0.7
- @pnpm/find-workspace-packages@3.0.4
- @pnpm/store-connection-manager@2.1.4
- @pnpm/filter-workspace-packages@4.0.3

## 4.1.4

### Patch Changes

- @pnpm/package-store@12.0.6
- supi@0.46.8
- @pnpm/cli-utils@0.6.3
- @pnpm/store-connection-manager@2.1.3
- @pnpm/find-workspace-packages@3.0.3
- @pnpm/plugin-commands-rebuild@4.0.2
- @pnpm/outdated@8.0.6
- @pnpm/filter-workspace-packages@4.0.2
- @pnpm/config@12.2.0

## 4.1.3

### Patch Changes

- Updated dependencies [66dbd06e6]
- Updated dependencies [3b147ced9]
  - supi@0.46.7
  - @pnpm/package-store@12.0.5
  - @pnpm/store-connection-manager@2.1.2
  - @pnpm/outdated@8.0.5
  - @pnpm/plugin-commands-rebuild@4.0.1

## 4.1.2

### Patch Changes

- Updated dependencies [3e3c3ff71]
- Updated dependencies [e6a2654a2]
  - supi@0.46.6
  - @pnpm/plugin-commands-rebuild@4.0.0
  - @pnpm/package-store@12.0.4
  - @pnpm/filter-workspace-packages@4.0.1
  - @pnpm/outdated@8.0.4
  - @pnpm/store-connection-manager@2.1.1
  - @pnpm/config@12.2.0

## 4.1.1

### Patch Changes

- supi@0.46.5

## 4.1.0

### Minor Changes

- 05baaa6e7: Add new config setting: `fetch-timeout`.

### Patch Changes

- Updated dependencies [97c64bae4]
- Updated dependencies [dfdf669e6]
- Updated dependencies [05baaa6e7]
- Updated dependencies [dfdf669e6]
- Updated dependencies [97c64bae4]
  - supi@0.46.4
  - @pnpm/filter-workspace-packages@4.0.0
  - @pnpm/config@12.2.0
  - @pnpm/store-connection-manager@2.1.0
  - @pnpm/common-cli-options-help@0.5.0
  - @pnpm/types@7.1.0
  - @pnpm/plugin-commands-rebuild@3.0.4
  - @pnpm/cli-utils@0.6.2
  - @pnpm/outdated@8.0.3
  - @pnpm/package-store@12.0.3
  - @pnpm/find-workspace-packages@3.0.2
  - @pnpm/manifest-utils@2.0.1
  - @pnpm/pnpmfile@1.0.1
  - @pnpm/resolver-base@8.0.1
  - @pnpm/sort-packages@2.0.1

## 4.0.3

### Patch Changes

- Updated dependencies [ba5231ccf]
  - @pnpm/config@12.1.0
  - @pnpm/cli-utils@0.6.1
  - @pnpm/plugin-commands-rebuild@3.0.3
  - @pnpm/store-connection-manager@2.0.3
  - @pnpm/find-workspace-packages@3.0.1
  - supi@0.46.3
  - @pnpm/filter-workspace-packages@3.0.1

## 4.0.2

### Patch Changes

- Updated dependencies [6f198457d]
- Updated dependencies [e3d9b3215]
- Updated dependencies [c70c77f89]
  - @pnpm/package-store@12.0.2
  - supi@0.46.2
  - @pnpm/store-connection-manager@2.0.2
  - @pnpm/plugin-commands-rebuild@3.0.2
  - @pnpm/outdated@8.0.2
  - @pnpm/config@12.0.0

## 4.0.1

### Patch Changes

- @pnpm/outdated@8.0.1
- @pnpm/plugin-commands-rebuild@3.0.1
- supi@0.46.1
- @pnpm/package-store@12.0.1
- @pnpm/store-connection-manager@2.0.1

## 4.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.
- 09e950fdc: `pnpmfile.js` renamed to `.pnpmfile.cjs`.
- 0d381e1d6: The default depth of an update is Infinity, not 0.
- aed712455: The --global option should be used when linking from/to the global modules directory.

### Minor Changes

- 46e71ea4a: `pnpm prune` should remove the modules cache.
- 735d2ac79: add new command `pnpm fetch`

### Patch Changes

- 7adc6e875: Update dependencies.
- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [78470a32d]
- Updated dependencies [78470a32d]
- Updated dependencies [f2d3b6c8b]
- Updated dependencies [945dc9f56]
- Updated dependencies [09e950fdc]
- Updated dependencies [aed712455]
- Updated dependencies [048c94871]
- Updated dependencies [78470a32d]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [83645c8ed]
- Updated dependencies [aed712455]
- Updated dependencies [7adc6e875]
- Updated dependencies [735d2ac79]
- Updated dependencies [9e30b9659]
  - @pnpm/constants@5.0.0
  - @pnpm/cli-utils@0.6.0
  - @pnpm/command@2.0.0
  - @pnpm/common-cli-options-help@0.4.0
  - @pnpm/config@12.0.0
  - @pnpm/error@2.0.0
  - @pnpm/filter-workspace-packages@3.0.0
  - @pnpm/find-workspace-dir@3.0.0
  - @pnpm/find-workspace-packages@3.0.0
  - @pnpm/manifest-utils@2.0.0
  - @pnpm/outdated@8.0.0
  - @pnpm/package-store@12.0.0
  - @pnpm/parse-wanted-dependency@2.0.0
  - @pnpm/plugin-commands-rebuild@3.0.0
  - @pnpm/pnpmfile@1.0.0
  - @pnpm/resolver-base@8.0.0
  - @pnpm/sort-packages@2.0.0
  - @pnpm/store-connection-manager@1.1.0
  - supi@0.46.0
  - @pnpm/types@7.0.0

## 3.5.28

### Patch Changes

- 4f1ce907a: Allow `--https-proxy`, `--proxy`, and `--noproxy` CLI options with the `install`, `add`, `update` commands.
- Updated dependencies [4f1ce907a]
  - @pnpm/config@11.14.2
  - @pnpm/cli-utils@0.5.4
  - @pnpm/plugin-commands-rebuild@2.2.34
  - @pnpm/store-connection-manager@1.0.4
  - @pnpm/find-workspace-packages@2.3.42
  - supi@0.45.4
  - @pnpm/filter-workspace-packages@2.3.14

## 3.5.27

### Patch Changes

- Updated dependencies [4b3852c39]
  - @pnpm/config@11.14.1
  - @pnpm/plugin-commands-rebuild@2.2.33
  - supi@0.45.3
  - @pnpm/cli-utils@0.5.3
  - @pnpm/store-connection-manager@1.0.3
  - @pnpm/find-workspace-packages@2.3.41
  - @pnpm/package-store@11.0.3
  - @pnpm/filter-workspace-packages@2.3.13

## 3.5.26

### Patch Changes

- @pnpm/plugin-commands-rebuild@2.2.32
- supi@0.45.2
- @pnpm/outdated@7.2.29
- @pnpm/package-store@11.0.2
- @pnpm/store-connection-manager@1.0.2

## 3.5.25

### Patch Changes

- Updated dependencies [632352f26]
  - @pnpm/package-store@11.0.1
  - @pnpm/store-connection-manager@1.0.1
  - supi@0.45.1
  - @pnpm/plugin-commands-rebuild@2.2.31

## 3.5.24

### Patch Changes

- Updated dependencies [f008425cd]
- Updated dependencies [8d1dfa89c]
  - supi@0.45.0
  - @pnpm/package-store@11.0.0
  - @pnpm/store-connection-manager@1.0.0
  - @pnpm/plugin-commands-rebuild@2.2.30
  - @pnpm/outdated@7.2.28
  - @pnpm/config@11.14.0
  - @pnpm/cli-utils@0.5.2
  - @pnpm/find-workspace-packages@2.3.40
  - @pnpm/filter-workspace-packages@2.3.12

## 3.5.23

### Patch Changes

- Updated dependencies [3be2b1773]
  - @pnpm/cli-utils@0.5.1
  - @pnpm/find-workspace-packages@2.3.39
  - @pnpm/plugin-commands-rebuild@2.2.29
  - supi@0.44.8
  - @pnpm/filter-workspace-packages@2.3.11

## 3.5.22

### Patch Changes

- @pnpm/outdated@7.2.27
- supi@0.44.7
- @pnpm/plugin-commands-rebuild@2.2.28

## 3.5.21

### Patch Changes

- 27a40321c: Update dependencies.
- Updated dependencies [27a40321c]
  - @pnpm/store-connection-manager@0.3.64
  - @pnpm/plugin-commands-rebuild@2.2.27
  - supi@0.44.6
  - @pnpm/outdated@7.2.26
  - @pnpm/package-store@10.1.18

## 3.5.20

### Patch Changes

- @pnpm/plugin-commands-rebuild@2.2.26
- supi@0.44.5

## 3.5.19

### Patch Changes

- Updated dependencies [249c068dd]
- Updated dependencies [a5e9d903c]
- Updated dependencies [cb040ae18]
  - @pnpm/outdated@7.2.25
  - @pnpm/common-cli-options-help@0.3.1
  - @pnpm/cli-utils@0.5.0
  - @pnpm/config@11.14.0
  - supi@0.44.4
  - @pnpm/plugin-commands-rebuild@2.2.25
  - @pnpm/find-workspace-packages@2.3.38
  - @pnpm/store-connection-manager@0.3.63
  - @pnpm/filter-workspace-packages@2.3.10

## 3.5.18

### Patch Changes

- Updated dependencies [ad113645b]
- Updated dependencies [c4cc62506]
  - supi@0.44.3
  - @pnpm/config@11.13.0
  - @pnpm/cli-utils@0.4.51
  - @pnpm/plugin-commands-rebuild@2.2.24
  - @pnpm/store-connection-manager@0.3.62
  - @pnpm/find-workspace-packages@2.3.37
  - @pnpm/outdated@7.2.24
  - @pnpm/package-store@10.1.17
  - @pnpm/filter-workspace-packages@2.3.9

## 3.5.17

### Patch Changes

- Updated dependencies [bff84dbca]
  - @pnpm/config@11.12.1
  - @pnpm/cli-utils@0.4.50
  - @pnpm/plugin-commands-rebuild@2.2.23
  - @pnpm/store-connection-manager@0.3.61
  - @pnpm/find-workspace-packages@2.3.36
  - supi@0.44.2
  - @pnpm/filter-workspace-packages@2.3.8

## 3.5.16

### Patch Changes

- @pnpm/cli-utils@0.4.49
- @pnpm/find-workspace-packages@2.3.35
- @pnpm/plugin-commands-rebuild@2.2.22
- @pnpm/filter-workspace-packages@2.3.7

## 3.5.15

### Patch Changes

- Updated dependencies [43de80034]
  - @pnpm/store-connection-manager@0.3.60
  - @pnpm/cli-utils@0.4.48
  - @pnpm/plugin-commands-rebuild@2.2.21
  - @pnpm/find-workspace-packages@2.3.34
  - @pnpm/filter-workspace-packages@2.3.6

## 3.5.14

### Patch Changes

- Updated dependencies [9a9bc67d2]
  - supi@0.44.1
  - @pnpm/plugin-commands-rebuild@2.2.20

## 3.5.13

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [548f28df9]
- Updated dependencies [548f28df9]
  - supi@0.44.0
  - @pnpm/types@6.4.0
  - @pnpm/cli-utils@0.4.47
  - @pnpm/config@11.12.0
  - @pnpm/outdated@7.2.23
  - @pnpm/find-workspace-packages@2.3.33
  - @pnpm/manifest-utils@1.1.5
  - @pnpm/package-store@10.1.16
  - @pnpm/plugin-commands-rebuild@2.2.19
  - @pnpm/pnpmfile@0.1.21
  - @pnpm/resolver-base@7.1.1
  - @pnpm/sort-packages@1.0.16
  - @pnpm/store-connection-manager@0.3.59
  - @pnpm/filter-workspace-packages@2.3.5

## 3.5.12

### Patch Changes

- Updated dependencies [941c5e8de]
  - @pnpm/global-bin-dir@1.2.6
  - supi@0.43.29
  - @pnpm/config@11.11.1
  - @pnpm/cli-utils@0.4.46
  - @pnpm/plugin-commands-rebuild@2.2.18
  - @pnpm/store-connection-manager@0.3.58
  - @pnpm/find-workspace-packages@2.3.32
  - @pnpm/filter-workspace-packages@2.3.4

## 3.5.11

### Patch Changes

- b653866c8: Fix the error message that happens when trying to add a new dependency to the root of a workspace.
- Updated dependencies [af897c324]
- Updated dependencies [af897c324]
  - supi@0.43.28
  - @pnpm/outdated@7.2.22
  - @pnpm/plugin-commands-rebuild@2.2.17

## 3.5.10

### Patch Changes

- supi@0.43.27

## 3.5.9

### Patch Changes

- Updated dependencies [f40bc5927]
- Updated dependencies [672c27cfe]
  - @pnpm/config@11.11.0
  - supi@0.43.26
  - @pnpm/outdated@7.2.21
  - @pnpm/cli-utils@0.4.45
  - @pnpm/plugin-commands-rebuild@2.2.16
  - @pnpm/store-connection-manager@0.3.57
  - @pnpm/find-workspace-packages@2.3.31
  - @pnpm/filter-workspace-packages@2.3.3

## 3.5.8

### Patch Changes

- supi@0.43.25

## 3.5.7

### Patch Changes

- 425c7547d: The real path of linked package should be used when installing its dependencies.
- 409736b4d: Linking dependencies by absolute path should work.
- Updated dependencies [425c7547d]
- Updated dependencies [32c9ef4be]
  - @pnpm/config@11.10.2
  - @pnpm/filter-workspace-packages@2.3.2
  - @pnpm/outdated@7.2.20
  - @pnpm/plugin-commands-rebuild@2.2.15
  - supi@0.43.24
  - @pnpm/cli-utils@0.4.44
  - @pnpm/store-connection-manager@0.3.56
  - @pnpm/package-store@10.1.15
  - @pnpm/find-workspace-packages@2.3.30

## 3.5.6

### Patch Changes

- Updated dependencies [ea09da716]
  - @pnpm/config@11.10.1
  - @pnpm/cli-utils@0.4.43
  - @pnpm/plugin-commands-rebuild@2.2.14
  - @pnpm/store-connection-manager@0.3.55
  - @pnpm/find-workspace-packages@2.3.29
  - supi@0.43.23
  - @pnpm/filter-workspace-packages@2.3.1

## 3.5.5

### Patch Changes

- Updated dependencies [1ec47db33]
- Updated dependencies [a8656b42f]
- Updated dependencies [ec37069f2]
  - @pnpm/common-cli-options-help@0.3.0
  - @pnpm/filter-workspace-packages@2.3.0
  - @pnpm/config@11.10.0
  - @pnpm/find-workspace-dir@2.0.0
  - @pnpm/plugin-commands-rebuild@2.2.13
  - @pnpm/cli-utils@0.4.42
  - @pnpm/store-connection-manager@0.3.54
  - @pnpm/find-workspace-packages@2.3.28
  - supi@0.43.22

## 3.5.4

### Patch Changes

- Updated dependencies [041537bc3]
  - @pnpm/config@11.9.1
  - supi@0.43.21
  - @pnpm/cli-utils@0.4.41
  - @pnpm/plugin-commands-rebuild@2.2.12
  - @pnpm/store-connection-manager@0.3.53
  - @pnpm/find-workspace-packages@2.3.27
  - @pnpm/filter-workspace-packages@2.2.13

## 3.5.3

### Patch Changes

- supi@0.43.20
- @pnpm/outdated@7.2.19
- @pnpm/package-store@10.1.14
- @pnpm/store-connection-manager@0.3.52
- @pnpm/plugin-commands-rebuild@2.2.11

## 3.5.2

### Patch Changes

- Updated dependencies [dc5a0a102]
- Updated dependencies [54ab5c87f]
  - @pnpm/store-connection-manager@0.3.51
  - @pnpm/filter-workspace-packages@2.2.12
  - @pnpm/plugin-commands-rebuild@2.2.10
  - @pnpm/outdated@7.2.18
  - supi@0.43.19
  - @pnpm/package-store@10.1.13

## 3.5.1

### Patch Changes

- @pnpm/outdated@7.2.17
- @pnpm/package-store@10.1.12
- @pnpm/store-connection-manager@0.3.50
- supi@0.43.18
- @pnpm/plugin-commands-rebuild@2.2.9

## 3.5.0

### Minor Changes

- 8698a7060: New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0
  - @pnpm/resolver-base@7.1.0
  - supi@0.43.17
  - @pnpm/cli-utils@0.4.40
  - @pnpm/plugin-commands-rebuild@2.2.8
  - @pnpm/store-connection-manager@0.3.49
  - @pnpm/package-store@10.1.11
  - @pnpm/find-workspace-packages@2.3.26
  - @pnpm/outdated@7.2.16
  - @pnpm/filter-workspace-packages@2.2.11

## 3.4.7

### Patch Changes

- Updated dependencies [fcc1c7100]
  - @pnpm/config@11.8.0
  - @pnpm/cli-utils@0.4.39
  - @pnpm/plugin-commands-rebuild@2.2.7
  - @pnpm/store-connection-manager@0.3.48
  - supi@0.43.16
  - @pnpm/find-workspace-packages@2.3.25
  - @pnpm/outdated@7.2.15
  - @pnpm/package-store@10.1.10
  - @pnpm/filter-workspace-packages@2.2.10

## 3.4.6

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/cli-utils@0.4.38
  - @pnpm/config@11.7.2
  - @pnpm/filter-workspace-packages@2.2.9
  - @pnpm/global-bin-dir@1.2.5
  - @pnpm/manifest-utils@1.1.4
  - @pnpm/outdated@7.2.14
  - @pnpm/pnpmfile@0.1.20
  - @pnpm/store-connection-manager@0.3.47
  - supi@0.43.15
  - @pnpm/package-store@10.1.9
  - @pnpm/find-workspace-packages@2.3.24
  - @pnpm/plugin-commands-rebuild@2.2.6

## 3.4.5

### Patch Changes

- @pnpm/outdated@7.2.13
- supi@0.43.14
- @pnpm/plugin-commands-rebuild@2.2.5

## 3.4.4

### Patch Changes

- Updated dependencies [09492b7b4]
  - @pnpm/package-store@10.1.8
  - @pnpm/outdated@7.2.12
  - supi@0.43.13
  - @pnpm/plugin-commands-rebuild@2.2.4
  - @pnpm/store-connection-manager@0.3.46
  - @pnpm/cli-utils@0.4.37
  - @pnpm/find-workspace-packages@2.3.23
  - @pnpm/filter-workspace-packages@2.2.8

## 3.4.3

### Patch Changes

- e70232907: Use @arcanis/slice-ansi instead of slice-ansi.
- Updated dependencies [c4ec56eeb]
  - supi@0.43.12
  - @pnpm/outdated@7.2.11
  - @pnpm/plugin-commands-rebuild@2.2.3
  - @pnpm/cli-utils@0.4.36
  - @pnpm/package-store@10.1.7
  - @pnpm/store-connection-manager@0.3.45
  - @pnpm/find-workspace-packages@2.3.22
  - @pnpm/filter-workspace-packages@2.2.7

## 3.4.2

### Patch Changes

- Updated dependencies [01aecf038]
  - @pnpm/package-store@10.1.6
  - supi@0.43.11
  - @pnpm/store-connection-manager@0.3.44
  - @pnpm/plugin-commands-rebuild@2.2.2
  - @pnpm/outdated@7.2.10

## 3.4.1

### Patch Changes

- Updated dependencies [b5d694e7f]
- Updated dependencies [c03a2b2cb]
  - supi@0.43.10
  - @pnpm/types@6.3.1
  - @pnpm/cli-utils@0.4.35
  - @pnpm/config@11.7.1
  - @pnpm/find-workspace-packages@2.3.21
  - @pnpm/manifest-utils@1.1.3
  - @pnpm/outdated@7.2.9
  - @pnpm/package-store@10.1.5
  - @pnpm/plugin-commands-rebuild@2.2.1
  - @pnpm/pnpmfile@0.1.19
  - @pnpm/resolver-base@7.0.5
  - @pnpm/sort-packages@1.0.15
  - @pnpm/store-connection-manager@0.3.43
  - @pnpm/filter-workspace-packages@2.2.6

## 3.4.0

### Minor Changes

- 50b360ec1: A new option added for specifying the shell to use, when running scripts: scriptShell.

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0
  - @pnpm/plugin-commands-rebuild@2.2.0
  - supi@0.43.9
  - @pnpm/cli-utils@0.4.34
  - @pnpm/store-connection-manager@0.3.42
  - @pnpm/find-workspace-packages@2.3.20
  - @pnpm/filter-workspace-packages@2.2.5

## 3.3.8

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0
  - supi@0.43.8
  - @pnpm/cli-utils@0.4.33
  - @pnpm/config@11.6.1
  - @pnpm/find-workspace-packages@2.3.19
  - @pnpm/manifest-utils@1.1.2
  - @pnpm/outdated@7.2.8
  - @pnpm/package-store@10.1.4
  - @pnpm/plugin-commands-rebuild@2.1.6
  - @pnpm/pnpmfile@0.1.18
  - @pnpm/resolver-base@7.0.4
  - @pnpm/sort-packages@1.0.14
  - @pnpm/store-connection-manager@0.3.41
  - @pnpm/filter-workspace-packages@2.2.4

## 3.3.7

### Patch Changes

- @pnpm/plugin-commands-rebuild@2.1.5
- supi@0.43.7

## 3.3.6

### Patch Changes

- supi@0.43.6

## 3.3.5

### Patch Changes

- 3a83db407: Update mem to v8.
- Updated dependencies [f591fdeeb]
- Updated dependencies [ddd98dd74]
- Updated dependencies [3a83db407]
- Updated dependencies [f591fdeeb]
  - @pnpm/config@11.6.0
  - supi@0.43.5
  - @pnpm/plugin-commands-rebuild@2.1.4
  - @pnpm/cli-utils@0.4.32
  - @pnpm/store-connection-manager@0.3.40
  - @pnpm/outdated@7.2.7
  - @pnpm/package-store@10.1.3
  - @pnpm/find-workspace-packages@2.3.18
  - @pnpm/filter-workspace-packages@2.2.3

## 3.3.4

### Patch Changes

- Updated dependencies [fb92e9f88]
  - supi@0.43.4
  - @pnpm/cli-utils@0.4.31
  - @pnpm/plugin-commands-rebuild@2.1.3
  - @pnpm/find-workspace-packages@2.3.17
  - @pnpm/filter-workspace-packages@2.2.2
  - @pnpm/outdated@7.2.6
  - @pnpm/package-store@10.1.2
  - @pnpm/store-connection-manager@0.3.39

## 3.3.3

### Patch Changes

- Updated dependencies [95ad9cafa]
  - supi@0.43.3

## 3.3.2

### Patch Changes

- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0
  - supi@0.43.2
  - @pnpm/cli-utils@0.4.30
  - @pnpm/plugin-commands-rebuild@2.1.2
  - @pnpm/store-connection-manager@0.3.38
  - @pnpm/find-workspace-packages@2.3.16
  - @pnpm/filter-workspace-packages@2.2.1

## 3.3.1

### Patch Changes

- Updated dependencies [a11aff299]
- Updated dependencies [9e774ae20]
  - @pnpm/filter-workspace-packages@2.2.0
  - supi@0.43.1
  - @pnpm/plugin-commands-rebuild@2.1.1
  - @pnpm/outdated@7.2.5
  - @pnpm/package-store@10.1.1
  - @pnpm/store-connection-manager@0.3.37

## 3.3.0

### Minor Changes

- 23cf3c88b: New option added: `shellEmulator`.

### Patch Changes

- Updated dependencies [23cf3c88b]
- Updated dependencies [846887de3]
  - @pnpm/config@11.4.0
  - @pnpm/plugin-commands-rebuild@2.1.0
  - supi@0.43.0
  - @pnpm/global-bin-dir@1.2.4
  - @pnpm/cli-utils@0.4.29
  - @pnpm/store-connection-manager@0.3.36
  - @pnpm/find-workspace-packages@2.3.15
  - @pnpm/filter-workspace-packages@2.1.22

## 3.2.2

### Patch Changes

- Updated dependencies [40a9e1f3f]
- Updated dependencies [0a6544043]
  - supi@0.42.0
  - @pnpm/package-store@10.1.0
  - @pnpm/store-connection-manager@0.3.35
  - @pnpm/plugin-commands-rebuild@2.0.41
  - @pnpm/outdated@7.2.4

## 3.2.1

### Patch Changes

- Updated dependencies [d94b19b39]
  - @pnpm/package-store@10.0.2
  - @pnpm/store-connection-manager@0.3.34
  - supi@0.41.31
  - @pnpm/plugin-commands-rebuild@2.0.40

## 3.2.0

### Minor Changes

- 092f8dd83: When the --workspace-root option is used, it is allowed to add a new dependency to the root workspace project. Because this way the intention is clear.

### Patch Changes

- Updated dependencies [7f74cd173]
- Updated dependencies [767212f4e]
- Updated dependencies [092f8dd83]
- Updated dependencies [092f8dd83]
  - @pnpm/package-store@10.0.1
  - @pnpm/config@11.3.0
  - @pnpm/common-cli-options-help@0.2.0
  - @pnpm/store-connection-manager@0.3.33
  - supi@0.41.30
  - @pnpm/cli-utils@0.4.28
  - @pnpm/plugin-commands-rebuild@2.0.39
  - @pnpm/find-workspace-packages@2.3.14
  - @pnpm/filter-workspace-packages@2.1.21

## 3.1.21

### Patch Changes

- Updated dependencies [86cd72de3]
- Updated dependencies [86cd72de3]
  - supi@0.41.29
  - @pnpm/package-store@10.0.0
  - @pnpm/manifest-utils@1.1.1
  - @pnpm/plugin-commands-rebuild@2.0.38
  - @pnpm/pnpmfile@0.1.17
  - @pnpm/store-connection-manager@0.3.32
  - @pnpm/outdated@7.2.3
  - @pnpm/cli-utils@0.4.27
  - @pnpm/find-workspace-packages@2.3.13
  - @pnpm/filter-workspace-packages@2.1.20

## 3.1.20

### Patch Changes

- Updated dependencies [6457562c4]
- Updated dependencies [968c26470]
- Updated dependencies [6457562c4]
  - @pnpm/package-store@9.1.8
  - @pnpm/plugin-commands-rebuild@2.0.37
  - supi@0.41.28
  - @pnpm/store-connection-manager@0.3.31
  - @pnpm/outdated@7.2.2

## 3.1.19

### Patch Changes

- Updated dependencies [e2f6b40b1]
- Updated dependencies [e2f6b40b1]
- Updated dependencies [5a3420ee5]
- Updated dependencies [e2f6b40b1]
  - @pnpm/manifest-utils@1.1.0
  - supi@0.41.27
  - @pnpm/cli-utils@0.4.26
  - @pnpm/outdated@7.2.1
  - @pnpm/find-workspace-packages@2.3.12
  - @pnpm/plugin-commands-rebuild@2.0.36
  - @pnpm/filter-workspace-packages@2.1.19

## 3.1.18

### Patch Changes

- Updated dependencies [1c2a8e03d]
  - @pnpm/outdated@7.2.0

## 3.1.17

### Patch Changes

- Updated dependencies [11dea936a]
  - supi@0.41.26
  - @pnpm/package-store@9.1.7
  - @pnpm/store-connection-manager@0.3.30
  - @pnpm/plugin-commands-rebuild@2.0.35

## 3.1.16

### Patch Changes

- Updated dependencies [c4165dccb]
  - supi@0.41.25

## 3.1.15

### Patch Changes

- Updated dependencies [c7e856fac]
  - supi@0.41.24

## 3.1.14

### Patch Changes

- Updated dependencies [8242401c7]
  - supi@0.41.23

## 3.1.13

### Patch Changes

- Updated dependencies [75a36deba]
- Updated dependencies [9f1a29ff9]
- Updated dependencies [8351fce25]
  - @pnpm/error@1.3.1
  - @pnpm/config@11.2.7
  - supi@0.41.22
  - @pnpm/cli-utils@0.4.25
  - @pnpm/filter-workspace-packages@2.1.18
  - @pnpm/global-bin-dir@1.2.3
  - @pnpm/outdated@7.1.12
  - @pnpm/pnpmfile@0.1.16
  - @pnpm/store-connection-manager@0.3.29
  - @pnpm/plugin-commands-rebuild@2.0.34
  - @pnpm/find-workspace-packages@2.3.11
  - @pnpm/package-store@9.1.6

## 3.1.12

### Patch Changes

- e65e9bb3d: It should be possible to set the fetch related options through CLI options.
  These are the fetch options:

  - `--fetch-retries=<number>`
  - `--fetch-retry-factor=<number>`
  - `--fetch-retry-maxtimeout=<number>`
  - `--fetch-retry-mintimeout=<number>`

- 6138b56d0: Update table to v6.
- Updated dependencies [83e2e6879]
  - supi@0.41.21

## 3.1.11

### Patch Changes

- 6cc36c85c: `pnpm install -r` should recreate the modules directory
  if the hoisting patterns were updated in a local config file.
  The hoisting patterns are configured via the `hoist-pattern`
  and `public-hoist-pattern` settings.
- 3feae5342: The same code should run when running some command inside a project directory, or when using `--filter` to select a specific workspace project.
- Updated dependencies [ac0d3e122]
  - @pnpm/config@11.2.6
  - @pnpm/cli-utils@0.4.24
  - @pnpm/plugin-commands-rebuild@2.0.33
  - @pnpm/store-connection-manager@0.3.28
  - @pnpm/find-workspace-packages@2.3.10
  - @pnpm/filter-workspace-packages@2.1.17

## 3.1.10

### Patch Changes

- Updated dependencies [4d4d22b63]
- Updated dependencies [972864e0d]
  - @pnpm/global-bin-dir@1.2.2
  - @pnpm/config@11.2.5
  - @pnpm/outdated@7.1.11
  - @pnpm/package-store@9.1.5
  - @pnpm/store-connection-manager@0.3.27
  - supi@0.41.20
  - @pnpm/cli-utils@0.4.23
  - @pnpm/plugin-commands-rebuild@2.0.32
  - @pnpm/find-workspace-packages@2.3.9
  - @pnpm/filter-workspace-packages@2.1.16

## 3.1.9

### Patch Changes

- Updated dependencies [999f81305]
- Updated dependencies [6d480dd7a]
  - @pnpm/filter-workspace-packages@2.1.15
  - @pnpm/find-workspace-dir@1.0.1
  - @pnpm/error@1.3.0
  - @pnpm/plugin-commands-rebuild@2.0.31
  - @pnpm/package-store@9.1.4
  - supi@0.41.19
  - @pnpm/cli-utils@0.4.22
  - @pnpm/config@11.2.4
  - @pnpm/global-bin-dir@1.2.1
  - @pnpm/outdated@7.1.10
  - @pnpm/pnpmfile@0.1.15
  - @pnpm/store-connection-manager@0.3.26
  - @pnpm/find-workspace-packages@2.3.8

## 3.1.8

### Patch Changes

- Updated dependencies [13c18e397]
  - @pnpm/config@11.2.3
  - @pnpm/cli-utils@0.4.21
  - @pnpm/plugin-commands-rebuild@2.0.30
  - @pnpm/store-connection-manager@0.3.25
  - @pnpm/find-workspace-packages@2.3.7
  - @pnpm/filter-workspace-packages@2.1.14

## 3.1.7

### Patch Changes

- Updated dependencies [9b90591e4]
- Updated dependencies [3f6d35997]
  - supi@0.41.18
  - @pnpm/config@11.2.2
  - @pnpm/cli-utils@0.4.20
  - @pnpm/plugin-commands-rebuild@2.0.29
  - @pnpm/store-connection-manager@0.3.24
  - @pnpm/find-workspace-packages@2.3.6
  - @pnpm/filter-workspace-packages@2.1.13

## 3.1.6

### Patch Changes

- cbbbe7a43: Fixes a regression introduced by <https://github.com/pnpm/pnpm/pull/2692>. `pnpm update` should update the direct dependencies of the project.

## 3.1.5

### Patch Changes

- @pnpm/cli-utils@0.4.19
- supi@0.41.17
- @pnpm/find-workspace-packages@2.3.5
- @pnpm/plugin-commands-rebuild@2.0.28
- @pnpm/filter-workspace-packages@2.1.12
- @pnpm/outdated@7.1.9
- @pnpm/package-store@9.1.3
- @pnpm/store-connection-manager@0.3.23

## 3.1.4

### Patch Changes

- Updated dependencies [0a8ff3ad3]
  - supi@0.41.16
  - @pnpm/cli-utils@0.4.18
  - @pnpm/find-workspace-packages@2.3.4
  - @pnpm/plugin-commands-rebuild@2.0.27
  - @pnpm/filter-workspace-packages@2.1.11
  - @pnpm/outdated@7.1.8
  - @pnpm/package-store@9.1.2
  - @pnpm/store-connection-manager@0.3.22

## 3.1.3

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [103ad7487]
- Updated dependencies [a2ef8084f]
  - supi@0.41.15
  - @pnpm/config@11.2.1
  - @pnpm/filter-workspace-packages@2.1.10
  - @pnpm/find-workspace-packages@2.3.3
  - @pnpm/package-store@9.1.1
  - @pnpm/plugin-commands-rebuild@2.0.26
  - @pnpm/outdated@7.1.7
  - @pnpm/cli-utils@0.4.17
  - @pnpm/store-connection-manager@0.3.21

## 3.1.2

### Patch Changes

- @pnpm/plugin-commands-rebuild@2.0.25
- supi@0.41.14

## 3.1.1

### Patch Changes

- supi@0.41.13

## 3.1.0

### Minor Changes

- 8c1cf25b7: Allow to update specific packages up until a specified depth. For instance, `pnpm update @types/* --depth Infinity`.

### Patch Changes

- Updated dependencies [8c1cf25b7]
  - supi@0.41.12

## 3.0.7

### Patch Changes

- Updated dependencies [ad69677a7]
- Updated dependencies [ad69677a7]
  - @pnpm/cli-utils@0.4.16
  - @pnpm/config@11.2.0
  - @pnpm/global-bin-dir@1.2.0
  - @pnpm/find-workspace-packages@2.3.2
  - @pnpm/plugin-commands-rebuild@2.0.24
  - @pnpm/store-connection-manager@0.3.20
  - @pnpm/filter-workspace-packages@2.1.9

## 3.0.6

### Patch Changes

- 7e47ebfb7: Allow to use `--save-workspace-protocol` with the install/update commands.
- Updated dependencies [a01626668]
  - supi@0.41.11
  - @pnpm/plugin-commands-rebuild@2.0.23

## 3.0.5

### Patch Changes

- Updated dependencies [9a908bc07]
  - @pnpm/package-store@9.1.0
  - @pnpm/plugin-commands-rebuild@2.0.22
  - @pnpm/pnpmfile@0.1.14
  - supi@0.41.10
  - @pnpm/store-connection-manager@0.3.19
  - @pnpm/outdated@7.1.6
  - @pnpm/cli-utils@0.4.15
  - @pnpm/find-workspace-packages@2.3.1
  - @pnpm/filter-workspace-packages@2.1.8

## 3.0.4

### Patch Changes

- 98e579270: `pnpm prune` should accept the `--[no-]optional`, `--[no-]dev` options.
- Updated dependencies [faae9a93c]
- Updated dependencies [65b4d07ca]
- Updated dependencies [ab3b8f51d]
- Updated dependencies [7b98d16c8]
  - @pnpm/find-workspace-packages@2.3.0
  - @pnpm/config@11.1.0
  - @pnpm/outdated@7.1.5
  - @pnpm/store-connection-manager@0.3.18
  - @pnpm/filter-workspace-packages@2.1.7
  - @pnpm/plugin-commands-rebuild@2.0.21
  - @pnpm/cli-utils@0.4.14
  - supi@0.41.9
  - @pnpm/package-store@9.0.14

## 3.0.3

### Patch Changes

- Updated dependencies [d9310c034]
  - @pnpm/store-connection-manager@0.3.17
  - @pnpm/plugin-commands-rebuild@2.0.20
  - @pnpm/outdated@7.1.4
  - @pnpm/package-store@9.0.13
  - supi@0.41.8

## 3.0.2

### Patch Changes

- Updated dependencies [1d8ec7208]
  - supi@0.41.7
  - @pnpm/plugin-commands-rebuild@2.0.19

## 3.0.1

### Patch Changes

- Updated dependencies [245221baa]
  - @pnpm/global-bin-dir@1.1.1
  - @pnpm/config@11.0.1
  - @pnpm/cli-utils@0.4.13
  - @pnpm/plugin-commands-rebuild@2.0.18
  - @pnpm/store-connection-manager@0.3.16
  - @pnpm/find-workspace-packages@2.2.11
  - @pnpm/filter-workspace-packages@2.1.6

## 3.0.0

### Major Changes

- 915828b46: A new setting is returned by `@pnpm/config`: `npmGlobalBinDir`.
  `npmGlobalBinDir` is the global executable directory used by npm.

  This new config is used by `@pnpm/global-bin-dir` to find a suitable
  directory for the binstubs installed by pnpm globally.

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [915828b46]
- Updated dependencies [915828b46]
  - @pnpm/config@11.0.0
  - @pnpm/global-bin-dir@1.1.0
  - @pnpm/cli-utils@0.4.12
  - @pnpm/plugin-commands-rebuild@2.0.17
  - @pnpm/store-connection-manager@0.3.15
  - @pnpm/outdated@7.1.3
  - @pnpm/package-store@9.0.12
  - supi@0.41.6
  - @pnpm/find-workspace-packages@2.2.10
  - @pnpm/filter-workspace-packages@2.1.5

## 2.1.6

### Patch Changes

- @pnpm/package-store@9.0.11
- @pnpm/store-connection-manager@0.3.14
- supi@0.41.5
- @pnpm/plugin-commands-rebuild@2.0.16

## 2.1.5

### Patch Changes

- Updated dependencies [2c190d49d]
  - @pnpm/global-bin-dir@1.0.1
  - @pnpm/config@10.0.1
  - @pnpm/cli-utils@0.4.11
  - @pnpm/plugin-commands-rebuild@2.0.15
  - @pnpm/store-connection-manager@0.3.13
  - @pnpm/find-workspace-packages@2.2.9
  - @pnpm/filter-workspace-packages@2.1.4

## 2.1.4

### Patch Changes

- 220896511: Remove common-tags from dependencies.
- Updated dependencies [db17f6f7b]
- Updated dependencies [c85768310]
- Updated dependencies [1146b76d2]
- Updated dependencies [1146b76d2]
- Updated dependencies [db17f6f7b]
- Updated dependencies [220896511]
  - @pnpm/config@10.0.0
  - @pnpm/outdated@7.1.2
  - @pnpm/global-bin-dir@1.0.0
  - @pnpm/types@6.2.0
  - @pnpm/plugin-commands-rebuild@2.0.14
  - supi@0.41.4
  - @pnpm/cli-utils@0.4.10
  - @pnpm/store-connection-manager@0.3.12
  - @pnpm/find-workspace-packages@2.2.8
  - @pnpm/manifest-utils@1.0.3
  - @pnpm/package-store@9.0.10
  - @pnpm/pnpmfile@0.1.13
  - @pnpm/resolver-base@7.0.3
  - @pnpm/sort-packages@1.0.13
  - @pnpm/filter-workspace-packages@2.1.3

## 2.1.3

### Patch Changes

- Updated dependencies [57d08f303]
- Updated dependencies [1adacd41e]
  - supi@0.41.3
  - @pnpm/package-store@9.0.9
  - @pnpm/store-connection-manager@0.3.11
  - @pnpm/plugin-commands-rebuild@2.0.13

## 2.1.2

### Patch Changes

- Updated dependencies [17b598c18]
- Updated dependencies [1520e3d6f]
  - supi@0.41.2
  - @pnpm/find-workspace-packages@2.2.7
  - @pnpm/package-store@9.0.8
  - @pnpm/filter-workspace-packages@2.1.2
  - @pnpm/plugin-commands-rebuild@2.0.12
  - @pnpm/outdated@7.1.1
  - @pnpm/store-connection-manager@0.3.10

## 2.1.1

### Patch Changes

- supi@0.41.1

## 2.1.0

### Minor Changes

- 71a8c8ce3: Added a new setting: `public-hoist-pattern`. This setting can be overwritten by `--[no-]shamefully-hoist`. The default value of `public-hoist-pattern` is `types/*`.

### Patch Changes

- Updated dependencies [6808c43fa]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/outdated@7.1.0
  - @pnpm/types@6.1.0
  - @pnpm/config@9.2.0
  - supi@0.41.0
  - @pnpm/cli-utils@0.4.9
  - @pnpm/find-workspace-packages@2.2.6
  - @pnpm/manifest-utils@1.0.2
  - @pnpm/package-store@9.0.7
  - @pnpm/plugin-commands-rebuild@2.0.11
  - @pnpm/pnpmfile@0.1.12
  - @pnpm/resolver-base@7.0.2
  - @pnpm/filter-workspace-packages@2.1.1
  - @pnpm/sort-packages@1.0.12
  - @pnpm/store-connection-manager@0.3.9

## 2.0.14

### Patch Changes

- @pnpm/package-store@9.0.6
- supi@0.40.1
- @pnpm/store-connection-manager@0.3.8
- @pnpm/plugin-commands-rebuild@2.0.10

## 2.0.13

### Patch Changes

- Updated dependencies [e37a5a175]
  - @pnpm/filter-workspace-packages@2.1.0
  - @pnpm/plugin-commands-rebuild@2.0.9

## 2.0.12

### Patch Changes

- e934b1a48: Update chalk to v4.1.0.
- Updated dependencies [41d92948b]
- Updated dependencies [e934b1a48]
  - supi@0.40.0
  - @pnpm/cli-utils@0.4.8
  - @pnpm/pnpmfile@0.1.11
  - @pnpm/outdated@7.0.27
  - @pnpm/plugin-commands-rebuild@2.0.9
  - @pnpm/store-connection-manager@0.3.7
  - @pnpm/find-workspace-packages@2.2.5
  - @pnpm/filter-workspace-packages@2.0.18
  - @pnpm/package-store@9.0.5

## 2.0.11

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [0e7ec4533]
- Updated dependencies [13630c659]
- Updated dependencies [d3ddd023c]
  - supi@0.39.10
  - @pnpm/package-store@9.0.4
  - @pnpm/plugin-commands-rebuild@2.0.8
  - @pnpm/store-connection-manager@0.3.6
  - @pnpm/pnpmfile@0.1.10
  - @pnpm/outdated@7.0.26
  - @pnpm/cli-utils@0.4.7
  - @pnpm/find-workspace-packages@2.2.4
  - @pnpm/filter-workspace-packages@2.0.17

## 2.0.10

### Patch Changes

- @pnpm/package-store@9.0.3
- supi@0.39.9
- @pnpm/store-connection-manager@0.3.5
- @pnpm/plugin-commands-rebuild@2.0.7

## 2.0.9

### Patch Changes

- @pnpm/package-store@9.0.2
- @pnpm/outdated@7.0.25
- @pnpm/store-connection-manager@0.3.4
- supi@0.39.8
- @pnpm/plugin-commands-rebuild@2.0.6

## 2.0.8

### Patch Changes

- @pnpm/store-connection-manager@0.3.3
- @pnpm/plugin-commands-rebuild@2.0.5

## 2.0.7

### Patch Changes

- Updated dependencies [ffddf34a8]
- Updated dependencies [ffddf34a8]
- Updated dependencies [429c5a560]
  - @pnpm/common-cli-options-help@0.2.0
  - @pnpm/config@9.1.0
  - @pnpm/package-store@9.0.1
  - @pnpm/plugin-commands-rebuild@2.0.4
  - @pnpm/cli-utils@0.4.6
  - @pnpm/find-workspace-packages@2.2.3
  - @pnpm/outdated@7.0.24
  - @pnpm/sort-packages@1.0.11
  - @pnpm/store-connection-manager@0.3.2
  - supi@0.39.7
  - @pnpm/filter-workspace-packages@2.0.16

## 2.0.6

### Patch Changes

- Updated dependencies [2f9c7ca85]
- Updated dependencies [160975d62]
  - supi@0.39.6

## 2.0.5

### Patch Changes

- @pnpm/store-connection-manager@0.3.1
- supi@0.39.5
- @pnpm/plugin-commands-rebuild@2.0.3

## 2.0.4

### Patch Changes

- @pnpm/plugin-commands-rebuild@2.0.2
- supi@0.39.4

## 2.0.3

### Patch Changes

- Updated dependencies [71b0cb8fd]
  - supi@0.39.3

## 2.0.2

### Patch Changes

- Updated dependencies [327bfbf02]
  - supi@0.39.2
  - @pnpm/plugin-commands-rebuild@2.0.1

## 2.0.1

### Patch Changes

- supi@0.39.1

## 2.0.0

### Major Changes

- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- 9e2a5b827: `pnpm r` is not an alias of `pnpm remove`.
- e11019b89: Deprecate the resolution strategy setting. The fewer dependencies strategy is used always.
- 802d145fc: Remove `independent-leaves` support.
- 45fdcfde2: Locking is removed.

### Minor Changes

- 242cf8737: The `link-workspace-packages` setting may be set to `deep`. When using `deep`,
  workspace packages are linked into subdependencies, not only to direct dependencies of projects.
- f516d266c: Executables are saved into a separate directory inside the content-addressable storage.

### Patch Changes

- 083d78968: Allow referencing packages not under the directory containing `pnpm-workspace.yaml`.
- 6cbf18676: The add and install commands should accept setting the `modules-dir` setting.
- f453a5f46: Update version-selector-type to v3.
- Updated dependencies [b5f66c0f2]
- Updated dependencies [2e8ebabb2]
- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [cc8a3bd31]
- Updated dependencies [cbc2192f1]
- Updated dependencies [3f73eaf0c]
- Updated dependencies [f516d266c]
- Updated dependencies [ecf2c6b7d]
- Updated dependencies [da091c711]
- Updated dependencies [a7d20d927]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [242cf8737]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [c207d994f]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [4f5801b1c]
- Updated dependencies [a5febb913]
- Updated dependencies [c25cccdad]
- Updated dependencies [f453a5f46]
- Updated dependencies [9fbb74ecb]
  - @pnpm/constants@4.0.0
  - @pnpm/package-store@9.0.0
  - @pnpm/plugin-commands-rebuild@2.0.0
  - supi@0.39.0
  - @pnpm/config@9.0.0
  - @pnpm/store-connection-manager@0.3.0
  - @pnpm/types@6.0.0
  - @pnpm/cli-utils@0.4.5
  - @pnpm/command@1.0.1
  - @pnpm/common-cli-options-help@0.1.6
  - @pnpm/error@1.2.1
  - @pnpm/filter-workspace-packages@2.0.15
  - @pnpm/find-workspace-dir@1.0.1
  - @pnpm/find-workspace-packages@2.2.2
  - @pnpm/manifest-utils@1.0.1
  - @pnpm/outdated@7.0.23
  - @pnpm/parse-wanted-dependency@1.0.1
  - @pnpm/pnpmfile@0.1.9
  - @pnpm/resolver-base@7.0.1
  - @pnpm/sort-packages@1.0.10

## 2.0.0-alpha.7

### Major Changes

- 45fdcfde2: Locking is removed.

### Minor Changes

- 242cf8737: The `link-workspace-packages` setting may be set to `deep`. When using `deep`,
  workspace packages are linked into subdependencies, not only to direct dependencies of projects.

### Patch Changes

- 083d78968: Allow referencing packages not under the directory containing `pnpm-workspace.yaml`.
- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [cc8a3bd31]
- Updated dependencies [a7d20d927]
- Updated dependencies [242cf8737]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [c25cccdad]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/constants@4.0.0-alpha.1
  - supi@0.39.0-alpha.7
  - @pnpm/package-store@9.0.0-alpha.5
  - @pnpm/plugin-commands-rebuild@2.0.0-alpha.5
  - @pnpm/store-connection-manager@0.3.0-alpha.5
  - @pnpm/cli-utils@0.4.5-alpha.2
  - @pnpm/find-workspace-packages@2.2.2-alpha.2
  - @pnpm/outdated@7.0.23-alpha.3
  - @pnpm/sort-packages@1.0.10-alpha.2
  - @pnpm/filter-workspace-packages@2.0.15-alpha.2

## 2.0.0-alpha.6

### Major Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.

### Patch Changes

- Updated dependencies [3f73eaf0]
- Updated dependencies [ecf2c6b7]
- Updated dependencies [da091c71]
- Updated dependencies [9fbb74ec]
  - @pnpm/plugin-commands-rebuild@2.0.0-alpha.4
  - supi@0.39.0-alpha.6
  - @pnpm/package-store@9.0.0-alpha.4
  - @pnpm/store-connection-manager@0.3.0-alpha.4
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/outdated@7.0.23-alpha.2
  - @pnpm/cli-utils@0.4.5-alpha.1
  - @pnpm/config@8.3.1-alpha.1
  - @pnpm/find-workspace-packages@2.2.2-alpha.1
  - @pnpm/manifest-utils@1.0.1-alpha.0
  - @pnpm/pnpmfile@0.1.9-alpha.0
  - @pnpm/resolver-base@7.0.1-alpha.0
  - @pnpm/sort-packages@1.0.10-alpha.1
  - @pnpm/filter-workspace-packages@2.0.15-alpha.1

## 2.0.0-alpha.5

### Patch Changes

- supi@0.38.30-alpha.5

## 2.0.0-alpha.4

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/package-store@9.0.0-alpha.3
  - @pnpm/plugin-commands-rebuild@2.0.0-alpha.3
  - supi@0.39.0-alpha.4
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/find-workspace-packages@2.2.2-alpha.0
  - @pnpm/outdated@7.0.23-alpha.1
  - @pnpm/store-connection-manager@0.2.32-alpha.3
  - @pnpm/cli-utils@0.4.5-alpha.0
  - @pnpm/sort-packages@1.0.10-alpha.0
  - @pnpm/filter-workspace-packages@2.0.15-alpha.0

## 2.0.0-alpha.3

### Patch Changes

- f453a5f46: Update version-selector-type to v3.
- Updated dependencies [c207d994f]
- Updated dependencies [f453a5f46]
  - @pnpm/package-store@9.0.0-alpha.2
  - supi@0.39.0-alpha.3
  - @pnpm/plugin-commands-rebuild@1.0.11-alpha.2
  - @pnpm/store-connection-manager@0.2.32-alpha.2
  - @pnpm/outdated@7.0.23-alpha.0

## 2.0.0-alpha.2

### Major Changes

- 9e2a5b827: `pnpm r` is not an alias of `pnpm remove`.

### Patch Changes

- Updated dependencies [2e8ebabb2]
  - supi@0.39.0-alpha.2

## 1.3.0-alpha.1

### Minor Changes

- 4f62d0383: Executables are saved into a separate directory inside the content-addressable storage.

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/package-store@9.0.0-alpha.1
  - supi@0.39.0-alpha.1
  - @pnpm/store-connection-manager@0.2.32-alpha.1
  - @pnpm/plugin-commands-rebuild@1.0.11-alpha.1

## 1.2.4-alpha.0

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/package-store@9.0.0-alpha.0
  - @pnpm/store-connection-manager@0.3.0-alpha.0
  - supi@0.39.0-alpha.0
  - @pnpm/plugin-commands-rebuild@1.0.11-alpha.0

## 1.2.4

### Patch Changes

- Updated dependencies [760cc6664]
  - supi@0.38.30
  - @pnpm/plugin-commands-rebuild@1.0.11

## 1.2.3

### Patch Changes

- 907c63a48: Dependencies updated.
- 907c63a48: Dependencies updated.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - supi@0.38.29
  - @pnpm/outdated@7.0.22
  - @pnpm/store-connection-manager@0.2.31
  - @pnpm/package-store@8.1.0
  - @pnpm/plugin-commands-rebuild@1.0.10
  - @pnpm/filter-workspace-packages@2.0.14
  - @pnpm/cli-utils@0.4.4
  - @pnpm/find-workspace-packages@2.2.1
