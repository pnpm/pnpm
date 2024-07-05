# @pnpm/config

## 21.6.0

### Minor Changes

- 1b03682: Read authentication information from .npmrc in the current directory when running `dlx` [#7996](https://github.com/pnpm/pnpm/issues/7996).

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
- Updated dependencies [9c63679]
  - @pnpm/types@11.0.0
  - @pnpm/workspace.read-manifest@2.2.0
  - @pnpm/pnpmfile@6.0.6
  - @pnpm/read-project-manifest@6.0.4
  - @pnpm/catalogs.config@0.1.0

## 21.5.0

### Minor Changes

- 7c6c923: Some registries allow the exact same content to be published under different package names and/or versions. This breaks the validity checks of packages in the store. To avoid errors when verifying the names and versions of such packages in the store, you may now set the `strict-store-pkg-content-check` setting to `false` [#4724](https://github.com/pnpm/pnpm/issues/4724).
- 04b8363: The `getConfig` function from `@pnpm/config` now reads the `pnpm-workspace.yaml` file and stores `workspacePackagePatterns` in the `Config` object. An internal refactor was made in pnpm to reuse this value instead of re-reading `pnpm-workspace.yaml` multiple times.

### Patch Changes

- 7d10394: Fix parsing of config variables in Turkish locale. Example: recursive-install parameter has problems on parsing.
- d8eab39: Fix `package-manager-strict-version` missing in config [#8195](https://github.com/pnpm/pnpm/issues/8195).
- Updated dependencies [13e55b2]
- Updated dependencies [5d1ed94]
  - @pnpm/types@10.1.1
  - @pnpm/workspace.read-manifest@2.1.0
  - @pnpm/pnpmfile@6.0.5
  - @pnpm/read-project-manifest@6.0.3

## 21.4.0

### Minor Changes

- 47341e5: **Semi-breaking.** Dependency key names in the lockfile are shortened if they are longer than 1000 characters. We don't expect this change to affect many users. Affected users most probably can't run install successfully at the moment. This change is required to fix some edge cases in which installation fails with an out-of-memory error or "Invalid string length (RangeError: Invalid string length)" error. The max allowed length of the dependency key can be controlled with the `peers-suffix-max-length` setting [#8177](https://github.com/pnpm/pnpm/pull/8177).

### Patch Changes

- @pnpm/pnpmfile@6.0.4

## 21.3.0

### Minor Changes

- b7ca13f: If `package-manager-strict-version` is set to `true` pnpm will fail if its version will not exactly match the version in the `packageManager` field of `package.json`.

## 21.2.3

### Patch Changes

- @pnpm/pnpmfile@6.0.3

## 21.2.2

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/pnpmfile@6.0.2
  - @pnpm/read-project-manifest@6.0.2

## 21.2.1

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/pnpmfile@6.0.1
  - @pnpm/read-project-manifest@6.0.1

## 21.2.0

### Minor Changes

- 9719a42: New setting called `virtual-store-dir-max-length` added to modify the maximum allowed length of the directories inside `node_modules/.pnpm`. The default length is set to 120 characters. This setting is particularly useful on Windows, where there is a limit to the maximum length of a file path [#7355](https://github.com/pnpm/pnpm/issues/7355).

## 21.1.0

### Minor Changes

- e0f47f4: `pnpm config get` now prints a comma-separated list for an array value instead of nothing.

## 21.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- 2d9e3b8: Use the same directory for state files on macOS as on Linux (`~/.local/state/pnpm`).
- cfa33f1: The [`dedupe-injected-deps`](https://pnpm.io/npmrc#dedupe-injected-deps) setting is `true` by default.
- e748162: The default value of the `link-workspace-packages` setting changed from `true` to `false`. This means that by default, dependencies will be linked from workspace packages only when they are specified using the [workspace protocol](https://pnpm.io/workspaces#workspace-protocol-workspace).
- 2b89155: `enable-pre-post-scripts` is set to `true` by default. This means that when you run a script like `start`, `prestart` and `poststart` will also run.
- 60839fc: The default value of the [hoist-workspace-packages](https://pnpm.io/npmrc#hoist-workspace-packages) is `true`.

### Minor Changes

- 7733f3a: Added support for registry-scoped SSL configurations (cert, key, and ca). Three new settings supported: `<registryURL>:certfile`, `<registryURL>:keyfile`, and `<registryURL>:ca`. For instance:

  ```
  //registry.mycomp.com/:certfile=server-cert.pem
  //registry.mycomp.com/:keyfile=server-key.pem
  //registry.mycomp.com/:cafile=client-cert.pem
  ```

  Related issue: [#7427](https://github.com/pnpm/pnpm/issues/7427).
  Related PR: [#7626](https://github.com/pnpm/pnpm/pull/7626).

- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.
- 98566d9: Added cache for `pnpm dlx` [#5277](https://github.com/pnpm/pnpm/issues/5277).

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [c692f80]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [730929e]
  - @pnpm/types@10.0.0
  - @pnpm/error@6.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/read-project-manifest@6.0.0
  - @pnpm/git-utils@2.0.0
  - @pnpm/matcher@6.0.0
  - @pnpm/pnpmfile@6.0.0

## 20.4.2

### Patch Changes

- @pnpm/pnpmfile@5.0.20

## 20.4.1

### Patch Changes

- d9564e354: Resolve the current working directory to its real location before doing any operations [#6524](https://github.com/pnpm/pnpm/issues/6524).

## 20.4.0

### Minor Changes

- c597f72ec: A new option added for hoisting packages from the workspace. When `hoist-workspace-packages` is set to `true`, packages from the workspace are symlinked to either `<workspace_root>/node_modules/.pnpm/node_modules` or to `<workspace_root>/node_modules` depending on other hoisting settings (`hoist-pattern` and `public-hoist-pattern`) [#7451](https://github.com/pnpm/pnpm/pull/7451).

## 20.3.0

### Minor Changes

- 4e71066dd: Use `--fail-if-no-match` if you want the CLI fail if no packages were matched by the command [#7403](https://github.com/pnpm/pnpm/issues/7403).

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/types@9.4.2
  - @pnpm/pnpmfile@5.0.19
  - @pnpm/read-project-manifest@5.0.10

## 20.2.0

### Minor Changes

- 672c559e4: A new setting added for symlinking [injected dependencies](https://pnpm.io/package_json#dependenciesmetainjected) from the workspace, if their dependencies use the same peer dependencies as the dependent package. The setting is called `dedupe-injected-deps` [#7416](https://github.com/pnpm/pnpm/pull/7416).

### Patch Changes

- Updated dependencies
  - @pnpm/types@9.4.1
  - @pnpm/pnpmfile@5.0.18
  - @pnpm/read-project-manifest@5.0.9

## 20.1.2

### Patch Changes

- @pnpm/pnpmfile@5.0.17

## 20.1.1

### Patch Changes

- @pnpm/pnpmfile@5.0.16

## 20.1.0

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

- d6592964f: `rootProjectManifestDir` is a required field.

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/pnpmfile@5.0.15
  - @pnpm/read-project-manifest@5.0.8

## 20.0.0

### Major Changes

- ac5abd3ff: The paths in patchedDependencies passed to `@pnpm/core` are absolute.

### Patch Changes

- b60bb6cbe: Update which to v4.

## 19.2.1

### Patch Changes

- b1dd0ee58: Instead of `pnpm.overrides` replacing `resolutions`, the two are now merged. This is intended to make it easier to migrate from Yarn by allowing one to keep using `resolutions` for Yarn, but adding additional changes just for pnpm using `pnpm.overrides`.

## 19.2.0

### Minor Changes

- d774a3196: Add a new setting: rootProjectManifestDir.
- 832e28826: Add `disallow-workspace-cycles` option to error instead of warn about cyclic dependencies

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/pnpmfile@5.0.14
  - @pnpm/read-project-manifest@5.0.7

## 19.1.0

### Minor Changes

- ee328fd25: Add `--hide-reporter-prefix' option for `run` command to hide project name as prefix for lifecycle log outputs of running scripts [#7061](https://github.com/pnpm/pnpm/issues/7061).

## 19.0.3

### Patch Changes

- @pnpm/pnpmfile@5.0.13
- @pnpm/read-project-manifest@5.0.6

## 19.0.2

### Patch Changes

- @pnpm/pnpmfile@5.0.12

## 19.0.1

### Patch Changes

- @pnpm/pnpmfile@5.0.11

## 19.0.0

### Major Changes

- cb8bcc8df: The default value of the `resolution-mode` setting is changed to `highest`. This setting was changed to `lowest-direct` in v8.0.0 and some users were [not happy with the change](https://github.com/pnpm/pnpm/issues/6463). A [poll](https://x.com/pnpmjs/status/1693707270897517022) concluded that most of the users want the old behaviour (`resolution-mode` set to `highest` by default). This is a semi-breaking change but should not affect users that commit their lockfile [#6463](https://github.com/pnpm/pnpm/issues/6463).

### Patch Changes

- @pnpm/pnpmfile@5.0.10
- @pnpm/read-project-manifest@5.0.5

## 18.4.4

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/pnpmfile@5.0.9
  - @pnpm/read-project-manifest@5.0.4

## 18.4.3

### Patch Changes

- Updated dependencies [b4892acc5]
  - @pnpm/read-project-manifest@5.0.3

## 18.4.2

### Patch Changes

- e2d631217: Don't crash when the APPDATA env variable is not set on Windows [#6659](https://github.com/pnpm/pnpm/issues/6659).

## 18.4.1

### Patch Changes

- Updated dependencies [302ebffc5]
  - @pnpm/constants@7.1.1
  - @pnpm/error@5.0.2
  - @pnpm/pnpmfile@5.0.8
  - @pnpm/read-project-manifest@5.0.2

## 18.4.0

### Minor Changes

- 301b8e2da: A new setting, `exclude-links-from-lockfile`, is now supported. When enabled, specifiers of local linked dependencies won't be duplicated in the lockfile.

  This setting was primarily added for use by [Bit CLI](https://github.com/teambit/bit), which links core aspects to `node_modules` from external directories. As such, the locations may vary across different machines, resulting in the generation of lockfiles with differing locations.

### Patch Changes

- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
  - @pnpm/types@9.1.0
  - @pnpm/constants@7.1.0
  - @pnpm/pnpmfile@5.0.7
  - @pnpm/read-project-manifest@5.0.1
  - @pnpm/error@5.0.1

## 18.3.2

### Patch Changes

- 1de07a4af: Normalize current working directory on Windows [#6524](https://github.com/pnpm/pnpm/issues/6524).

## 18.3.1

### Patch Changes

- 2809e89ab: Make sure `--otp` option is in the publish's cli options [6384](https://github.com/pnpm/pnpm/issues/6384).

## 18.3.0

### Minor Changes

- 32f8e08c6: A custom compression level may be specified for the `pnpm pack` command using the `pack-gzip-level` setting [#6393](https://github.com/pnpm/pnpm/issues/6393).

### Patch Changes

- @pnpm/pnpmfile@5.0.6

## 18.2.0

### Minor Changes

- fc8780ca9: Allow env variables to be specified with default values in `.npmrc`. This is a convention used by Yarn too.
  Using `${NAME-fallback}` will return `fallback` if `NAME` isn't set. `${NAME:-fallback}` will return `fallback` if `NAME` isn't set, or is an empty string [#6018](https://github.com/pnpm/pnpm/issues/6018).

### Patch Changes

- @pnpm/pnpmfile@5.0.5

## 18.1.1

### Patch Changes

- @pnpm/pnpmfile@5.0.4

## 18.1.0

### Minor Changes

- e2cb4b63d: Add `ignore-workspace-cycles` to silence workspace cycle warning [#6308](https://github.com/pnpm/pnpm/pull/6308).
- cd6ce11f0: A new settig has been added called `dedupe-direct-deps`, which is disabled by default. When set to `true`, dependencies that are already symlinked to the root `node_modules` directory of the workspace will not be symlinked to subproject `node_modules` directories. This feature was enabled by default in v8.0.0 but caused issues, so it's best to disable it by default [#6299](https://github.com/pnpm/pnpm/issues/6299).

### Patch Changes

- @pnpm/pnpmfile@5.0.3

## 18.0.2

### Patch Changes

- @pnpm/pnpmfile@5.0.2

## 18.0.1

### Patch Changes

- @pnpm/pnpmfile@5.0.1

## 18.0.0

### Major Changes

- 47e45d717: `auto-install-peers` is `true` by default.
- 47e45d717: `save-workspace-protocol` is `rolling` by default.
- 158d8cf22: `useLockfileV6` field is deleted. Lockfile v5 cannot be written anymore, only transformed to the new format.
- eceaa8b8b: Node.js 14 support dropped.
- 8e35c21d1: Use lockfile v6 by default.
- 47e45d717: `resolve-peers-from-workspace-root` is `true` by default.
- 47e45d717: `publishConfig.linkDirectory` is `true` by default.
- 113f0ae26: `resolution-mode` is `lowest-direct` by default.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/read-project-manifest@5.0.0
  - @pnpm/constants@7.0.0
  - @pnpm/git-utils@1.0.0
  - @pnpm/matcher@5.0.0
  - @pnpm/pnpmfile@5.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0

## 17.0.2

### Patch Changes

- @pnpm/pnpmfile@4.0.40

## 17.0.1

### Patch Changes

- b38d711f3: `extend-node-path` is `true` by default. It was set to `false` in v7.29.2 but it appears that it was a breaking change [#6213](https://github.com/pnpm/pnpm/issues/6213).
  - @pnpm/pnpmfile@4.0.39

## 17.0.0

### Major Changes

- e505b58e3: Don't extend NODE_PATH in command shims [#5176](https://github.com/pnpm/pnpm/issues/5176).

### Patch Changes

- @pnpm/read-project-manifest@4.1.4
- @pnpm/pnpmfile@4.0.38

## 16.7.2

### Patch Changes

- @pnpm/pnpmfile@4.0.37

## 16.7.1

### Patch Changes

- @pnpm/pnpmfile@4.0.36

## 16.7.0

### Minor Changes

- 5c31fa8be: A new setting is now supported: `dedupe-peer-dependents`.

  When this setting is set to `true`, packages with peer dependencies will be deduplicated after peers resolution.

  For instance, let's say we have a workspace with two projects and both of them have `webpack` in their dependencies. `webpack` has `esbuild` in its optional peer dependencies, and one of the projects has `esbuild` in its dependencies. In this case, pnpm will link two instances of `webpack` to the `node_modules/.pnpm` directory: one with `esbuild` and another one without it:

  ```
  node_modules
    .pnpm
      webpack@1.0.0_esbuild@1.0.0
      webpack@1.0.0
  project1
    node_modules
      webpack -> ../../node_modules/.pnpm/webpack@1.0.0/node_modules/webpack
  project2
    node_modules
      webpack -> ../../node_modules/.pnpm/webpack@1.0.0_esbuild@1.0.0/node_modules/webpack
      esbuild
  ```

  This makes sense because `webpack` is used in two projects, and one of the projects doesn't have `esbuild`, so the two projects cannot share the same instance of `webpack`. However, this is not what most developers expect, especially since in a hoisted `node_modules`, there would only be one instance of `webpack`. Therefore, you may now use the `dedupe-peer-dependents` setting to deduplicate `webpack` when it has no conflicting peer dependencies. In this case, if we set `dedupe-peer-dependents` to `true`, both projects will use the same `webpack` instance, which is the one that has `esbuild` resolved:

  ```
  node_modules
    .pnpm
      webpack@1.0.0_esbuild@1.0.0
  project1
    node_modules
      webpack -> ../../node_modules/.pnpm/webpack@1.0.0_esbuild@1.0.0/node_modules/webpack
  project2
    node_modules
      webpack -> ../../node_modules/.pnpm/webpack@1.0.0_esbuild@1.0.0/node_modules/webpack
      esbuild
  ```

### Patch Changes

- @pnpm/pnpmfile@4.0.35

## 16.6.4

### Patch Changes

- @pnpm/pnpmfile@4.0.34

## 16.6.3

### Patch Changes

- @pnpm/pnpmfile@4.0.33

## 16.6.2

### Patch Changes

- @pnpm/pnpmfile@4.0.32

## 16.6.1

### Patch Changes

- @pnpm/pnpmfile@4.0.31

## 16.6.0

### Minor Changes

- 59ee53678: A new `resolution-mode` added: `lowest-direct`. With this resolution mode direct dependencies will be resolved to their lowest versions. So if there is `foo@^1.1.0` in the dependencies, then `1.1.0` will be installed, even if the latest version of `foo` is `1.2.0`.

### Patch Changes

- @pnpm/pnpmfile@4.0.30

## 16.5.5

### Patch Changes

- @pnpm/pnpmfile@4.0.29

## 16.5.4

### Patch Changes

- @pnpm/pnpmfile@4.0.28

## 16.5.3

### Patch Changes

- @pnpm/pnpmfile@4.0.27

## 16.5.2

### Patch Changes

- @pnpm/pnpmfile@4.0.26

## 16.5.1

### Patch Changes

- @pnpm/pnpmfile@4.0.25

## 16.5.0

### Minor Changes

- 28b47a156: When `extend-node-path` is set to `false`, the `NODE_PATH` environment variable is not set in the command shims [#5910](https://github.com/pnpm/pnpm/pull/5910)

### Patch Changes

- @pnpm/pnpmfile@4.0.24

## 16.4.3

### Patch Changes

- @pnpm/pnpmfile@4.0.23

## 16.4.2

### Patch Changes

- @pnpm/pnpmfile@4.0.22

## 16.4.1

### Patch Changes

- @pnpm/pnpmfile@4.0.21

## 16.4.0

### Minor Changes

- 3ebce5db7: Added support for `pnpm-lock.yaml` format v6. This new format will be the new lockfile format in pnpm v8. To use the new lockfile format, use the `use-lockfile-v6=true` setting in `.npmrc`. Or run `pnpm install --use-lockfile-v6` [#5810](https://github.com/pnpm/pnpm/pull/5810).

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/constants@6.2.0
  - @pnpm/pnpmfile@4.0.20
  - @pnpm/error@4.0.1
  - @pnpm/read-project-manifest@4.1.3

## 16.3.0

### Minor Changes

- 1fad508b0: When the `resolve-peers-from-workspace-root` setting is set to `true`, pnpm will use dependencies installed in the root of the workspace to resolve peer dependencies in any of the workspace's projects [#5882](https://github.com/pnpm/pnpm/pull/5882).

### Patch Changes

- @pnpm/pnpmfile@4.0.19

## 16.2.2

### Patch Changes

- @pnpm/pnpmfile@4.0.18

## 16.2.1

### Patch Changes

- d71dbf230: Only the `pnpm add --global <pkg>` command should fail if there is no global pnpm bin directory in the system PATH [#5841](https://github.com/pnpm/pnpm/issues/5841).

## 16.2.0

### Minor Changes

- 841f52e70: pnpm reads settings from its own global configuration file at `$XDG_CONFIG_HOME/pnpm/rc` [#5829](https://github.com/pnpm/pnpm/pull/5829).

## 16.1.11

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/pnpmfile@4.0.17
  - @pnpm/read-project-manifest@4.1.2

## 16.1.10

### Patch Changes

- @pnpm/pnpmfile@4.0.16

## 16.1.9

### Patch Changes

- @pnpm/pnpmfile@4.0.15

## 16.1.8

### Patch Changes

- @pnpm/pnpmfile@4.0.14

## 16.1.7

### Patch Changes

- a9d59d8bc: Update dependencies.
  - @pnpm/read-project-manifest@4.1.1
  - @pnpm/pnpmfile@4.0.13

## 16.1.6

### Patch Changes

- @pnpm/pnpmfile@4.0.12

## 16.1.5

### Patch Changes

- @pnpm/pnpmfile@4.0.11

## 16.1.4

### Patch Changes

- Updated dependencies [fec9e3149]
- Updated dependencies [0d12d38fd]
  - @pnpm/read-project-manifest@4.1.0
  - @pnpm/pnpmfile@4.0.10

## 16.1.3

### Patch Changes

- Updated dependencies [969f8a002]
  - @pnpm/matcher@4.0.1
  - @pnpm/pnpmfile@4.0.9

## 16.1.2

### Patch Changes

- @pnpm/pnpmfile@4.0.8

## 16.1.1

### Patch Changes

- @pnpm/pnpmfile@4.0.7

## 16.1.0

### Minor Changes

- 3dab7f83c: New function added: `readLocalConfig(dir: string)`.

### Patch Changes

- @pnpm/pnpmfile@4.0.6

## 16.0.5

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - @pnpm/pnpmfile@4.0.5
  - @pnpm/read-project-manifest@4.0.2

## 16.0.4

### Patch Changes

- @pnpm/pnpmfile@4.0.4

## 16.0.3

### Patch Changes

- aacb83f73: Print a warning if a package.json has a workspaces field but there is no pnpm-workspace.yaml file [#5363](https://github.com/pnpm/pnpm/issues/5363).
- a14ad09e6: It should be possible to set a custom home directory for pnpm by changing the PNPM_HOME environment variable.
  - @pnpm/pnpmfile@4.0.3

## 16.0.2

### Patch Changes

- bea0acdfc: Add `pnpm doctor` command to do checks for known common issues
  - @pnpm/pnpmfile@4.0.2

## 16.0.1

### Patch Changes

- e7fd8a84c: Downgrade `@pnpm/npm-conf` to remove annoying builtin warning [#5518](https://github.com/pnpm/pnpm/issues/5518).
- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - @pnpm/pnpmfile@4.0.1
  - @pnpm/read-project-manifest@4.0.1

## 16.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Minor Changes

- 645384bfd: New field returned: allProjectsGraph.

### Patch Changes

- 1d0fd82fd: Print a warning when cannot read the builtin npm configuration.
- 3c117996e: `strict-peer-dependencies` is set to `false` by default.
- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/matcher@4.0.0
  - @pnpm/pnpmfile@4.0.0
  - @pnpm/read-project-manifest@4.0.0

## 15.10.12

### Patch Changes

- @pnpm/pnpmfile@3.0.3
- @pnpm/read-project-manifest@3.0.13

## 15.10.11

### Patch Changes

- @pnpm/pnpmfile@3.0.2

## 15.10.10

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/pnpmfile@3.0.1
  - @pnpm/read-project-manifest@3.0.12

## 15.10.9

### Patch Changes

- Updated dependencies [abb41a626]
- Updated dependencies [d665f3ff7]
- Updated dependencies [51566e34b]
  - @pnpm/matcher@3.2.0
  - @pnpm/types@8.7.0
  - @pnpm/pnpmfile@3.0.0
  - @pnpm/read-project-manifest@3.0.11

## 15.10.8

### Patch Changes

- @pnpm/pnpmfile@2.2.12

## 15.10.7

### Patch Changes

- @pnpm/pnpmfile@2.2.11

## 15.10.6

### Patch Changes

- Updated dependencies [156cc1ef6]
- Updated dependencies [9b44d38a4]
  - @pnpm/types@8.6.0
  - @pnpm/matcher@3.1.0
  - @pnpm/pnpmfile@2.2.10
  - @pnpm/read-project-manifest@3.0.10

## 15.10.5

### Patch Changes

- @pnpm/pnpmfile@2.2.9

## 15.10.4

### Patch Changes

- @pnpm/pnpmfile@2.2.8

## 15.10.3

### Patch Changes

- @pnpm/pnpmfile@2.2.7

## 15.10.2

### Patch Changes

- @pnpm/pnpmfile@2.2.6

## 15.10.1

### Patch Changes

- @pnpm/pnpmfile@2.2.5

## 15.10.0

### Minor Changes

- 2aa22e4b1: Set `NODE_PATH` when `preferSymlinkedExecutables` is enabled.

### Patch Changes

- @pnpm/pnpmfile@2.2.4

## 15.9.4

### Patch Changes

- @pnpm/pnpmfile@2.2.3

## 15.9.3

### Patch Changes

- @pnpm/pnpmfile@2.2.2

## 15.9.2

### Patch Changes

- @pnpm/pnpmfile@2.2.1

## 15.9.1

### Patch Changes

- Updated dependencies [5035fdae1]
- Updated dependencies [23984abd1]
  - @pnpm/pnpmfile@2.2.0

## 15.9.0

### Minor Changes

- 43cd6aaca: When `ignore-dep-scripts` is `true`, ignore scripts of dependencies but run the scripts of the project.
- 65c4260de: Support a new hook for passing a custom package importer to the store controller.
- 29a81598a: When `ignore-compatibility-db` is set to `true`, the [compatibility database](https://github.com/yarnpkg/berry/blob/master/packages/yarnpkg-extensions/sources/index.ts) will not be used to patch dependencies [#5132](https://github.com/pnpm/pnpm/issues/5132).

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [39c040127]
- Updated dependencies [65c4260de]
  - @pnpm/read-project-manifest@3.0.9
  - @pnpm/pnpmfile@2.1.0

## 15.8.1

### Patch Changes

- 34121d753: Don't crash when a config file contains a setting with an env variable that doesn't exist [#5093](https://github.com/pnpm/pnpm/issues/5093).
- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/pnpmfile@2.0.9
  - @pnpm/read-project-manifest@3.0.8

## 15.8.0

### Minor Changes

- cac34ad69: `verify-store-integrity=false` makes pnpm skip checking the integrities of files in the global content-addressable store.
- 99019e071: Allow to set `only-built-dependencies[]` through `.npmrc`.

## 15.7.1

### Patch Changes

- @pnpm/pnpmfile@2.0.8

## 15.7.0

### Minor Changes

- 4fa1091c8: Add experimental lockfile format that should merge conflict less in the `importers` section. Enabled by setting the `use-inline-specifiers-lockfile-format = true` feature flag in `.npmrc`.

  If this feature flag is committed to a repo, we recommend setting the minimum allowed version of pnpm to this release in the `package.json` `engines` field. Once this is set, older pnpm versions will throw on invalid lockfile versions.

### Patch Changes

- Updated dependencies [01c5834bf]
  - @pnpm/read-project-manifest@3.0.7

## 15.6.1

### Patch Changes

- 7334b347b: Update npm-conf.

## 15.6.0

### Minor Changes

- 28f000509: A new setting supported: `prefer-symlinked-executables`. When `true`, pnpm will create symlinks to executables in
  `node_modules/.bin` instead of command shims (but on POSIX systems only).

  This setting is `true` by default when `node-linker` is set to `hoisted`.

  Related issue: [#4782](https://github.com/pnpm/pnpm/issues/4782).

### Patch Changes

- 406656f80: When `lockfile-include-tarball-url` is set to `true`, every entry in `pnpm-lock.yaml` will contain the full URL to the package's tarball [#5054](https://github.com/pnpm/pnpm/pull/5054).

## 15.5.2

### Patch Changes

- Updated dependencies [744d47d90]
  - @pnpm/pnpmfile@2.0.7

## 15.5.1

### Patch Changes

- 5f643f23b: Update ramda to v0.28.

## 15.5.0

### Minor Changes

- f48d46ef6: New setting added: `include-workspace-root`. When it is set to `true`, the `run`, `exec`, `add`, and `test` commands will include the root package, when executed recursively [#4906](https://github.com/pnpm/pnpm/issues/4906)

## 15.4.1

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/pnpmfile@2.0.6
  - @pnpm/read-project-manifest@3.0.6

## 15.4.0

### Minor Changes

- 47b5e45dd: `package-import-method` supports a new option: `clone-or-copy`.

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/pnpmfile@2.0.5
  - @pnpm/read-project-manifest@3.0.5

## 15.3.0

### Minor Changes

- 56cf04cb3: New settings added: use-git-branch-lockfile, merge-git-branch-lockfiles, merge-git-branch-lockfiles-branch-pattern.

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [56cf04cb3]
  - @pnpm/types@8.2.0
  - @pnpm/git-utils@0.1.0
  - @pnpm/pnpmfile@2.0.4
  - @pnpm/read-project-manifest@3.0.4

## 15.2.1

### Patch Changes

- 25798aad1: Don't fail when the cafile setting is specified [#4877](https://github.com/pnpm/pnpm/issues/4877). This fixes a regression introduced in pnpm v7.2.0.

## 15.2.0

### Minor Changes

- d5730ba81: The ca and cert options may accept an array of string.

### Patch Changes

- bc80631d3: Update npm-conf.
- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/pnpmfile@2.0.3
  - @pnpm/read-project-manifest@3.0.3

## 15.1.4

### Patch Changes

- ae2f845c5: `NODE_ENV=production pnpm install --dev` should only install dev deps [#4745](https://github.com/pnpm/pnpm/pull/4745).

## 15.1.3

### Patch Changes

- 05159665d: Do not return a default value for the node-version setting.

## 15.1.2

### Patch Changes

- af22c6c4f: When the global bin directory is set to a symlink, check not only the symlink in the PATH but also the target of the symlink [#4744](https://github.com/pnpm/pnpm/issues/4744).

## 15.1.1

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/pnpmfile@2.0.2
  - @pnpm/read-project-manifest@3.0.2

## 15.1.0

### Minor Changes

- e05dcc48a: New setting added to turn back v6 directory filtering that doesn't require globs: `legacy-dir-filtering`.

## 15.0.0

### Major Changes

- 546e644e9: Don't hoist types by default to the root of `node_modules` [#4459](https://github.com/pnpm/pnpm/pull/4459).
- 4bed585e2: The next deprecated settings were removed:

  - frozen-shrinkwrap
  - prefer-frozen-shrinkwrap
  - shared-workspace-shrinkwrap
  - shrinkwrap-directory
  - lockfile-directory
  - shrinkwrap-only
  - store

### Minor Changes

- 8dac029ef: Any package with "prettier" in its name is hoisted.
- c6463b9fd: New setting added: `git-shallow-hosts`. When cloning repositories from "shallow-hosts", pnpm will use shallow cloning to fetch only the needed commit, not all the history [#4548](https://github.com/pnpm/pnpm/pull/4548).
- 8fa95fd86: The default value of `nodeLinker` is set to `isolated`.

### Patch Changes

- 72b79f55a: Setting the `auto-install-peers` to `true` should work.
- Updated dependencies [1267e4eff]
  - @pnpm/constants@6.1.0
  - @pnpm/error@3.0.1
  - @pnpm/pnpmfile@2.0.1
  - @pnpm/read-project-manifest@3.0.1

## 14.0.0

### Major Changes

- 516859178: `extendNodePath` removed.
- 73d71a2d5: `strict-peer-dependencies` is `true` by default.
- fa656992c: The `embed-readme` setting is `false` by default.
- 542014839: Node.js 12 is not supported.
- 585e9ca9e: `pnpm install -g pkg` will add the global command only to a predefined location. pnpm will not try to add a bin to the global Node.js or npm folder. To set the global bin directory, either set the `PNPM_HOME` env variable or the [`global-bin-dir`](https://pnpm.io/npmrc#global-bin-dir) setting.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/error@3.0.0
  - @pnpm/pnpmfile@2.0.0
  - @pnpm/read-project-manifest@3.0.0

## 13.13.2

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/global-bin-dir@3.0.1
  - @pnpm/pnpmfile@1.2.6
  - @pnpm/read-project-manifest@2.0.13

## 13.13.1

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/pnpmfile@1.2.5
  - @pnpm/read-project-manifest@2.0.12

## 13.13.0

### Minor Changes

- 334e5340a: Add support of the `update-notifier` configuration option [#4158](https://github.com/pnpm/pnpm/issues/4158).

## 13.12.0

### Minor Changes

- b7566b979: embed-readme option was added

## 13.11.0

### Minor Changes

- fff0e4493: Set `side-effects-cache-read` and `side-effects-cache-write`.

## 13.10.0

### Minor Changes

- e76151f66: New setting supported: `auto-install-peers`. When it is set to `true`, `pnpm add <pkg>` automatically installs any missing peer dependencies as `devDependencies`.

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/pnpmfile@1.2.4
  - @pnpm/read-project-manifest@2.0.11

## 13.9.0

### Minor Changes

- 8fe8f5e55: New CLI option: `--ignore-workspace`. When used, pnpm ignores any workspace configuration found in the current or parent directories.

## 13.8.0

### Minor Changes

- 732d4962f: nodeLinker may accept two new values: `isolated` and `hoisted`.

  `hoisted` will create a "classic" `node_modules` folder without using symlinks.

  `isolated` will be the default value that creates a symlinked `node_modules`.

- a6cf11cb7: `userConfig` added to the config object, which contain only the settings set in the user's home config file.

## 13.7.2

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/pnpmfile@1.2.3
  - @pnpm/read-project-manifest@2.0.10

## 13.7.1

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/pnpmfile@1.2.2
  - @pnpm/read-project-manifest@2.0.9

## 13.7.0

### Minor Changes

- 927c4a089: A new option `--aggregate-output` for `append-only` reporter is added. It aggregates lifecycle logs output for each command that is run in parallel, and only prints command logs when command is finished.

  Related discussion: [#4070](https://github.com/pnpm/pnpm/discussions/4070).

- 10a4bd4db: New option added for: `node-mirror:<releaseDir>`. The string value of this dynamic option is used as the base URL for downloading node when `use-node-version` is specified. The `<releaseDir>` portion of this argument can be any dir in `https://nodejs.org/download`. Which `<releaseDir>` dynamic config option gets selected depends on the value of `use-node-version`. If 'use-node-version' is a simple `x.x.x` version string, `<releaseDir>` becomes `release` and `node-mirror:release` is read. Defaults to `https://nodejs.org/download/<releaseDir>/`.

### Patch Changes

- 30bfca967: When normalizing registry URLs, a trailing slash should only be added if the registry URL has no path.

  So `https://registry.npmjs.org` is changed to `https://registry.npmjs.org/` but `https://npm.pkg.github.com/owner` is unchanged.

  Related issue: [#4034](https://github.com/pnpm/pnpm/issues/4034).

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/pnpmfile@1.2.1
  - @pnpm/read-project-manifest@2.0.8

## 13.6.1

### Patch Changes

- 46aaf7108: Revert the change that was made in pnpm v6.23.2 causing a regression describe in [#4052](https://github.com/pnpm/pnpm/issues/4052).

## 13.6.0

### Minor Changes

- 8a99a01ff: Read the root project manifest and write it to the config object.

## 13.5.1

### Patch Changes

- a7ff2d5ce: When normalizing registry URLs, a trailing slash should only be added if the registry URL has no path.

  So `https://registry.npmjs.org` is changed to `https://registry.npmjs.org/` but `https://npm.pkg.github.com/owner` is unchanged.

  Related issue: [#4034](https://github.com/pnpm/pnpm/issues/4034).

## 13.5.0

### Minor Changes

- 002778559: New setting added: `scriptsPrependNodePath`. This setting can be `true`, `false`, or `warn-only`.
  When `true`, the path to the `node` executable with which pnpm executed is prepended to the `PATH` of the scripts.
  When `warn-only`, pnpm will print a warning if the scripts run with a `node` binary that differs from the `node` binary executing the pnpm CLI.

## 13.4.2

### Patch Changes

- Updated dependencies [302ae4f6f]
- Updated dependencies [b75993dde]
  - @pnpm/pnpmfile@1.2.0
  - @pnpm/types@7.6.0

## 13.4.1

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/pnpmfile@1.1.1

## 13.4.0

### Minor Changes

- b6d74c545: Allow a system's package manager to override pnpm's default settings

## 13.3.0

### Minor Changes

- bd7bcdbe8: New setting supported: maxsockets. maxsockets allows to set the maximum number of connections to use per origin (protocol/host/post combination).

## 13.2.0

### Minor Changes

- 5ee3b2dc7: New setting: `configDir`.

## 13.1.0

### Minor Changes

- 4027a3c69: New optional field added to the Config object: hooks.

### Patch Changes

- Updated dependencies [ef9d2719a]
- Updated dependencies [4027a3c69]
  - @pnpm/pnpmfile@1.1.0

## 13.0.0

### Major Changes

- c7081cbb4: NODE_PATH is not set in the command shims of globally installed packages.

### Minor Changes

- fe5688dc0: Add option 'changed-files-ignore-pattern' to ignore changed files by glob patterns when filtering for changed projects since the specified commit/branch.
- c7081cbb4: New option added: `extendNodePath`. When it is set to `false`, pnpm does not set the `NODE_PATH` environment variable in the command shims.

## 12.6.0

### Minor Changes

- d62259d67: Use a subfolder of the pnpm homedir as the location of globally installed packages, when use-beta-cli is on.

## 12.5.0

### Minor Changes

- 6681fdcbc: New setting added: `global-bin-dir`. `global-bin-dir` allows to set the target directory for the bin files of globally installed packages.

## 12.4.9

### Patch Changes

- ede519190: Fix a bug that doesn't respect `cache-dir`/`state-dir` paths from configuration files

## 12.4.8

### Patch Changes

- Updated dependencies [47a1e9696]
  - @pnpm/global-bin-dir@3.0.0

## 12.4.7

### Patch Changes

- 655af55ba: The default home directory for pnpm on macOS should be at `~/Library/pnpm`.

## 12.4.6

### Patch Changes

- 3fb74c618: Don't ignore the `--workspace-root` option.

## 12.4.5

### Patch Changes

- 051296a16: workspaceRoot should only be read for CLI options.

## 12.4.4

### Patch Changes

- af8b5716e: pnpm should always have write access to its home directory

## 12.4.3

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0

## 12.4.2

### Patch Changes

- 73c1f802e: Choose the right location for global dir.

## 12.4.1

### Patch Changes

- 2264bfdf4: Choose proper default state-dir and cache-dir on macOS.

## 12.4.0

### Minor Changes

- 25f6968d4: Add `workspace-concurrency` based on CPU cores amount, just set `workspace-concurrency` as zero or negative, the concurrency limit is set as `max((amount of cores) - abs(workspace-concurrency), 1)`
- 5aaf3e3fa: New setting added: stateDir.

## 12.3.3

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0

## 12.3.2

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0

## 12.3.1

### Patch Changes

- a1a03d145: Import only the required functions from ramda.

## 12.3.0

### Minor Changes

- 84ec82e05: New setting added: `use-node-version`. When set, pnpm will install the specified version of Node.js and use it for running any lifecycle scripts.
- c2a71e4fd: New CLI option added: `use-stderr`. When set, all the output is written to stderr.
- 84ec82e05: New settings are returned: pnpmExecPath and pnpmHomeDir.

## 12.2.0

### Minor Changes

- 05baaa6e7: Add new config setting: `fetch-timeout`.
- dfdf669e6: Add new cli arg --filter-prod. --filter-prod acts the same as --filter, but it omits devDependencies when building dependencies

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0

## 12.1.0

### Minor Changes

- ba5231ccf: New option added for: `enable-pre-post-scripts`. When it is set to `true`, lifecycle scripts with pre/post prefixes are automatically executed by pnpm.

## 12.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.
- aed712455: Remove `pnpm-prefix` setting support.
- aed712455: `globalDir` is never set. Only the `dir` option is set with the global directory location when the `--global` is used. The pnpm CLI should have access to the global dir, otherwise an exception is thrown.

### Minor Changes

- 78470a32d: New setting added: `modules-cache-max-age`. The default value of the setting is 10080 (7 days in seconds). `modules-cache-max-age` is the time in minutes after which pnpm should remove the orphan packages from node_modules.

### Patch Changes

- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [f2bb5cbeb]
  - @pnpm/constants@5.0.0
  - @pnpm/error@2.0.0
  - @pnpm/global-bin-dir@2.0.0
  - @pnpm/types@7.0.0

## 11.14.2

### Patch Changes

- 4f1ce907a: Add type for `noproxy`.

## 11.14.1

### Patch Changes

- 4b3852c39: The noproxy setting should work.

## 11.14.0

### Minor Changes

- cb040ae18: add option to check unknown settings

## 11.13.0

### Minor Changes

- c4cc62506: Add '--reverse' flag for reversing the order of package executions during 'recursive run'

## 11.12.1

### Patch Changes

- bff84dbca: fix: remove empty keys from config key check

## 11.12.0

### Minor Changes

- 548f28df9: print warnings if unknown settings are found in .npmrc

### Patch Changes

- Updated dependencies [9ad8c27bf]
  - @pnpm/types@6.4.0

## 11.11.1

### Patch Changes

- Updated dependencies [941c5e8de]
  - @pnpm/global-bin-dir@1.2.6

## 11.11.0

### Minor Changes

- f40bc5927: New option added: enableModulesDir. When `false`, pnpm will not write any files to the modules directory. This is useful for when you want to mount the modules directory with FUSE.

## 11.10.2

### Patch Changes

- 425c7547d: Always resolve the target directory to its real path.

## 11.10.1

### Patch Changes

- ea09da716: The test-pattern option should be an Array.

## 11.10.0

### Minor Changes

- a8656b42f: New option added: `test-pattern`. `test-pattern` allows to detect whether the modified files are related to tests. If they are, the dependent packages of such modified packages are not included.

## 11.9.1

### Patch Changes

- 041537bc3: Finding global bin directory on Windows.

## 11.9.0

### Minor Changes

- 8698a7060: New option added: preferWorkspacePackages. When it is `true`, dependencies are linked from the workspace even, when there are newer version available in the registry.

## 11.8.0

### Minor Changes

- fcc1c7100: Add prettier plugins to the default public-hoist-pattern list

## 11.7.2

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/global-bin-dir@1.2.5

## 11.7.1

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1

## 11.7.0

### Minor Changes

- 50b360ec1: A new option added for specifying the shell to use, when running scripts: scriptShell.

## 11.6.1

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0

## 11.6.0

### Minor Changes

- f591fdeeb: New option added: `node-linker`. When `node-linker` is set to `pnp`, pnpm will create a `.pnp.js` file.

## 11.5.0

### Minor Changes

- 74914c178: New experimental option added for installing node_modules w/o symlinks.

## 11.4.0

### Minor Changes

- 23cf3c88b: New option added: `shellEmulator`.

### Patch Changes

- Updated dependencies [846887de3]
  - @pnpm/global-bin-dir@1.2.4

## 11.3.0

### Minor Changes

- 092f8dd83: New setting added: workspace-root.

### Patch Changes

- 767212f4e: Packages like @babel/types should be publicly hoisted by default.

## 11.2.7

### Patch Changes

- 9f1a29ff9: During global install, changes should always be saved to the global package.json, even when save is set to false.
- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/global-bin-dir@1.2.3

## 11.2.6

### Patch Changes

- ac0d3e122: Publicly hoist any dependency that is related to ESLint.

## 11.2.5

### Patch Changes

- 972864e0d: When public-hoist-pattern is set to an empty string or a list with a single empty string, then it is considered to be undefined.
- Updated dependencies [4d4d22b63]
  - @pnpm/global-bin-dir@1.2.2

## 11.2.4

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/global-bin-dir@1.2.1

## 11.2.3

### Patch Changes

- 13c18e397: Stop searching for local prefix, when directory has a `package.json5` or `package.yaml`.

## 11.2.2

### Patch Changes

- 3f6d35997: Don't read the `.npmrc` files that are outside of the current workspace.

## 11.2.1

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 11.2.0

### Minor Changes

- ad69677a7: A new option added that allows to resolve the global bin directory from directories to which there is no write access.

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/global-bin-dir@1.2.0

## 11.1.0

### Minor Changes

- 65b4d07ca: feat: add config to make install only install package dependencies in a workspace
- ab3b8f51d: Hoist all ESLint plugins to the root of node_modules by default.

## 11.0.1

### Patch Changes

- Updated dependencies [245221baa]
  - @pnpm/global-bin-dir@1.1.1

## 11.0.0

### Major Changes

- 71aeb9a38: Remove proxy from the object returned by @pnpm/config. httpsProxy and httpProxy are returned instead.

### Minor Changes

- 915828b46: A new setting is returned by `@pnpm/config`: `npmGlobalBinDir`.
  `npmGlobalBinDir` is the global executable directory used by npm.

  This new config is used by `@pnpm/global-bin-dir` to find a suitable
  directory for the binstubs installed by pnpm globally.

### Patch Changes

- Updated dependencies [915828b46]
  - @pnpm/global-bin-dir@1.1.0

## 10.0.1

### Patch Changes

- Updated dependencies [2c190d49d]
  - @pnpm/global-bin-dir@1.0.1

## 10.0.0

### Major Changes

- db17f6f7b: Move Project and ProjectsGraph to @pnpm/types.
- 1146b76d2: `globalBin` is removed from the returned object.

  The value of `bin` is set by the `@pnpm/global-bin-dir` package when the `--global` option is used.

### Patch Changes

- Updated dependencies [1146b76d2]
- Updated dependencies [db17f6f7b]
  - @pnpm/global-bin-dir@1.0.0
  - @pnpm/types@6.2.0

## 9.2.0

### Minor Changes

- 71a8c8ce3: Added a new setting: `public-hoist-pattern`. This setting can be overwritten by `--[no-]shamefully-hoist`. The default value of `public-hoist-pattern` is `types/*`.

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0

## 9.1.0

### Minor Changes

- ffddf34a8: Add new global option called `--stream`.
  When used, the output from child processes is streamed to the console immediately, prefixed with the originating package directory. This allows output from different packages to be interleaved.

## 9.0.0

### Major Changes

- e11019b89: Deprecate the resolution strategy setting. The fewer dependencies strategy is used always.
- 802d145fc: Remove `independent-leaves` support.
- 45fdcfde2: Locking is removed.

### Minor Changes

- 242cf8737: The `link-workspace-packages` setting may be set to `deep`. When using `deep`,
  workspace packages are linked into subdependencies, not only to direct dependencies of projects.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [da091c711]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/types@6.0.0
  - @pnpm/error@1.2.1

## 9.0.0-alpha.2

### Major Changes

- 45fdcfde2: Locking is removed.

### Minor Changes

- 242cf8737: The `link-workspace-packages` setting may be set to `deep`. When using `deep`,
  workspace packages are linked into subdependencies, not only to direct dependencies of projects.

### Patch Changes

- Updated dependencies [ca9f50844]
  - @pnpm/constants@4.0.0-alpha.1

## 8.3.1-alpha.1

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0

## 8.3.1-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
