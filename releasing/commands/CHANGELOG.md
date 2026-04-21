# @pnpm/releasing.commands

## 1100.2.0

### Minor Changes

- db81c32: `pnpm pack-app`: replaced the `--node-version` flag with `--runtime`, which takes a `<name>@<version>` spec (e.g. `--runtime node@22.0.0`). The corresponding `pnpm.app.nodeVersion` key in package.json was renamed to `pnpm.app.runtime` with the same syntax. Only `node` is supported today; the prefix leaves room for future runtimes (`bun`, `deno`).

  The previous `--node-version` flag silently inherited from pnpm's global `node-version` rc setting (which controls which Node runs user scripts), causing the wrong Node build to be embedded in SEAs for users who had that rc key set.

### Patch Changes

- Updated dependencies [421317c]
  - @pnpm/engine.runtime.node-resolver@1101.0.0
  - @pnpm/installing.client@1100.0.4
  - @pnpm/fetching.directory-fetcher@1100.0.4
  - @pnpm/engine.runtime.commands@1100.0.4
  - @pnpm/installing.commands@1100.1.2
  - @pnpm/exec.lifecycle@1100.0.4
  - @pnpm/fs.indexed-pkg-importer@1100.0.3
  - @pnpm/lockfile.fs@1100.0.3
  - @pnpm/config.reader@1101.1.1
  - @pnpm/releasing.exportable-manifest@1100.0.2
  - @pnpm/workspace.projects-filter@1100.0.4

## 1100.1.0

### Minor Changes

- 72c1e05: Added a new `pnpm pack-app` command that packs a CommonJS entry file into a standalone executable for one or more target platforms, using the [Node.js Single Executable Applications](https://nodejs.org/api/single-executable-applications.html) API under the hood. Targets are specified as `<os>-<arch>[-<libc>]` (e.g. `linux-x64`, `linux-x64-musl`, `macos-arm64`, `win-x64`) and each produces an executable under `dist-app/<target>/` by default. Requires Node.js v25.5+ to perform the injection; an older host downloads Node.js v25 automatically.
- 53668a4: Fixed and expanded `pnpm version` to match npm behavior:

  - Accept an explicit semver version (e.g. `pnpm version 1.2.3`) in addition to bump types.
  - Recognize `--no-commit-hooks`, `--no-git-tag-version`, `--sign-git-tag`, and `--message`.
  - Fix `--no-git-checks` which was previously parsed incorrectly.
  - Create a git commit and annotated tag for the version bump when running inside a git repository (unless `--no-git-tag-version` is used). `--message` supports `%s` replacement with the new version, and `--tag-version-prefix` controls the tag prefix (defaults to `v`). Git commits and tags are always skipped in recursive mode since multiple packages may be bumped to different versions in a single run [#11271](https://github.com/pnpm/pnpm/issues/11271).

### Patch Changes

- Updated dependencies [7d25bc1]
- Updated dependencies [e03e8f4]
- Updated dependencies [72c1e05]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0
  - @pnpm/fetching.directory-fetcher@1100.0.3
  - @pnpm/resolving.resolver-base@1100.1.0
  - @pnpm/engine.runtime.commands@1100.0.3
  - @pnpm/engine.runtime.node-resolver@1100.0.3
  - @pnpm/installing.commands@1100.1.1
  - @pnpm/exec.lifecycle@1100.0.3
  - @pnpm/installing.client@1100.0.3
  - @pnpm/lockfile.types@1100.0.2
  - @pnpm/lockfile.fs@1100.0.2
  - @pnpm/fs.indexed-pkg-importer@1100.0.2
  - @pnpm/workspace.projects-filter@1100.0.3
  - @pnpm/releasing.exportable-manifest@1100.0.2

## 1100.0.2

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/cli.utils@1101.0.0
  - @pnpm/config.reader@1101.0.0
  - @pnpm/installing.commands@1100.1.0
  - @pnpm/engine.runtime.commands@1100.0.2
  - @pnpm/workspace.projects-filter@1100.0.2
  - @pnpm/exec.lifecycle@1100.0.2
  - @pnpm/fetching.directory-fetcher@1100.0.2
  - @pnpm/releasing.exportable-manifest@1100.0.2
  - @pnpm/installing.client@1100.0.2

## 1100.0.1

### Patch Changes

- Internally, `@pnpm/network.web-auth`'s `promptBrowserOpen` now uses the [`open`](https://www.npmjs.com/package/open) package instead of spawning platform-specific commands. The `execFile` field and `PromptBrowserOpenExecFile` / `PromptBrowserOpenProcess` type exports have been removed from `PromptBrowserOpenContext`.
- Updated dependencies
- Updated dependencies [ff28085]
  - @pnpm/network.web-auth@1101.0.0
  - @pnpm/types@1101.0.0
  - @pnpm/bins.resolver@1100.0.1
  - @pnpm/cli.utils@1100.0.1
  - @pnpm/config.pick-registry-for-package@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/deps.path@1100.0.1
  - @pnpm/exec.lifecycle@1100.0.1
  - @pnpm/fetching.directory-fetcher@1100.0.1
  - @pnpm/installing.client@1100.0.1
  - @pnpm/installing.commands@1100.0.1
  - @pnpm/lockfile.fs@1100.0.1
  - @pnpm/lockfile.types@1100.0.1
  - @pnpm/network.fetch@1100.0.1
  - @pnpm/releasing.exportable-manifest@1100.0.1
  - @pnpm/resolving.resolver-base@1100.0.1
  - @pnpm/workspace.projects-filter@1100.0.1
  - @pnpm/workspace.projects-sorter@1100.0.1
  - @pnpm/fs.indexed-pkg-importer@1100.0.1
  - @pnpm/engine.runtime.commands@1100.0.1

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.
- 7b1c189: Removed the deprecated `allowNonAppliedPatches` completely in favor of `allowUnusedPatches`.
  Remove `ignorePatchFailures` so all patch application failures should throw an error.
- 71de2b3: Removed support for the `useNodeVersion` and `executionEnv.nodeVersion` fields. `devEngines.runtime` and `engines.runtime` should be used instead [#10373](https://github.com/pnpm/pnpm/pull/10373).
- cc7c0d2: `pnpm publish` now works without the `npm` CLI.

  The One-time Password feature now reads from `PNPM_CONFIG_OTP` instead of `NPM_CONFIG_OTP`:

  ```sh
  export PNPM_CONFIG_OTP='<your OTP here>'
  pnpm publish --no-git-checks
  ```

  If the registry requests OTP and the user has not provided it via the `PNPM_CONFIG_OTP` environment variable or the `--otp` flag, pnpm will prompt the user directly for an OTP code.

  If the registry requests web-based authentication, pnpm will print a scannable QR code along with the URL.

  Since the new `pnpm publish` no longer calls `npm publish`, some undocumented features may have been unknowingly dropped. If you rely on a feature that is now gone, please open an issue at <https://github.com/pnpm/pnpm/issues>. In the meantime, you can use `pnpm pack && npm publish *.tgz` as a workaround.

### Minor Changes

- cb367b9: Preserve `allowBuilds` settings when deploying a project. The `allowBuilds` configuration is now written to `pnpm-workspace.yaml` in the deploy directory.
- 144d76f: Added support for `--dry-run` to the `pack` command [#10301](https://github.com/pnpm/pnpm/issues/10301).
- d5be835: Implement `version` command natively in pnpm to support workspaces and workspace: protocols correctly. The new command allows bumping package versions (major, minor, patch, etc.) with full workspace support and git integration.
- 38b8e35: Support for custom resolvers and fetchers.

### Patch Changes

- 4c6c26a: When the [`enableGlobalVirtualStore`](https://pnpm.io/settings#enableglobalvirtualstore) option is set, the `pnpm deploy` command would incorrectly create symlinks to the global virtual store. To keep the deploy directory self-contained, `pnpm deploy` now ignores this setting and always creates a localized virtual store within the deploy directory.
- fea46dc: `pnpm publish -r --force` should allow to run publish over already existing versions in the registry [#10272](https://github.com/pnpm/pnpm/issues/10272).
- d4a1d73: Create `@pnpm/network.web-auth`.
- 8385a8c: Remove the `injectWorkspacePackages` setting from the lockfile on the `deploy` command [#10294](https://github.com/pnpm/pnpm/pull/10294).
- Updated dependencies [e1ea779]
- Updated dependencies [7730a7f]
- Updated dependencies [5f73b0f]
- Updated dependencies [449dacf]
- Updated dependencies [996284f]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [4c6c26a]
- Updated dependencies [de3dc74]
- Updated dependencies [c55c614]
- Updated dependencies [9b0a460]
- Updated dependencies [3c72b6b]
- Updated dependencies [9f5c0e3]
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [90bd3c3]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [29764fb]
- Updated dependencies [da2429d]
- Updated dependencies [9065f49]
- Updated dependencies [1fd7370]
- Updated dependencies [0b5ccc9]
- Updated dependencies [1cc61e8]
- Updated dependencies [d4a1d73]
- Updated dependencies [491a84f]
- Updated dependencies [9b1e5da]
- Updated dependencies [13855ac]
- Updated dependencies [62f760e]
- Updated dependencies [f0ae1b9]
- Updated dependencies [9fc552d]
- Updated dependencies [cbb366a]
- Updated dependencies [312226c]
- Updated dependencies [0dfa8b8]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [075aa99]
- Updated dependencies [23eb4a6]
- Updated dependencies [fd511e4]
- Updated dependencies [ae43ac7]
- Updated dependencies [d7b8be4]
- Updated dependencies [ccec8e7]
- Updated dependencies [fd511e4]
- Updated dependencies [fa5a5c6]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [0625e20]
- Updated dependencies [ee9fe58]
- Updated dependencies [d458ab3]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [96704a1]
- Updated dependencies [50fbeca]
- Updated dependencies [bb8baa7]
- Updated dependencies [4a36b9a]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [6c480a4]
- Updated dependencies [2efb5d2]
- Updated dependencies [6f806be]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [ace7903]
- Updated dependencies [38b8e35]
- Updated dependencies [1e6de25]
- Updated dependencies [831f574]
- Updated dependencies [366cabe]
- Updated dependencies [2df8b71]
- Updated dependencies [ed1a7fe]
- Updated dependencies [15549a9]
- Updated dependencies [60b5fd1]
- Updated dependencies [b51bb42]
- Updated dependencies [cc7c0d2]
- Updated dependencies [5bf7768]
- Updated dependencies [ae43ac7]
- Updated dependencies [a5fdbf9]
- Updated dependencies [9d3f00b]
- Updated dependencies [efb48dc]
- Updated dependencies [f03b9ec]
- Updated dependencies [6b3d87a]
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [3574905]
- Updated dependencies [f871365]
  - @pnpm/cli.common-cli-options-help@1001.0.0
  - @pnpm/config.reader@1005.0.0
  - @pnpm/deps.path@1002.0.0
  - @pnpm/bins.resolver@1001.0.0
  - @pnpm/installing.commands@1005.0.0
  - @pnpm/resolving.resolver-base@1006.0.0
  - @pnpm/network.web-auth@1001.0.0
  - @pnpm/constants@1002.0.0
  - @pnpm/types@1001.0.0
  - @pnpm/lockfile.fs@1002.0.0
  - @pnpm/lockfile.types@1003.0.0
  - @pnpm/cli.utils@1002.0.0
  - @pnpm/releasing.exportable-manifest@1001.0.0
  - @pnpm/engine.runtime.commands@1000.0.0
  - @pnpm/workspace.projects-filter@1001.0.0
  - @pnpm/config.pick-registry-for-package@1001.0.0
  - @pnpm/fetching.directory-fetcher@1001.0.0
  - @pnpm/fs.is-empty-dir-or-nothing@1001.0.0
  - @pnpm/fs.indexed-pkg-importer@1001.0.0
  - @pnpm/workspace.projects-sorter@1001.0.0
  - @pnpm/network.git-utils@1001.0.0
  - @pnpm/installing.client@1002.0.0
  - @pnpm/catalogs.types@1001.0.0
  - @pnpm/exec.lifecycle@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/network.fetch@1001.0.0
  - @pnpm/fs.packlist@1001.0.0
