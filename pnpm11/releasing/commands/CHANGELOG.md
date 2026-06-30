# @pnpm/releasing.commands

## 1100.5.2

### Patch Changes

- Updated dependencies [25a829e]
- Updated dependencies [bae694f]
- Updated dependencies [05b95ab]
- Updated dependencies [6545793]
- Updated dependencies [0ec878d]
- Updated dependencies [852d537]
  - @pnpm/installing.commands@1100.10.1
  - @pnpm/resolving.resolver-base@1100.5.0
  - @pnpm/network.fetch@1100.1.4
  - @pnpm/engine.runtime.node-resolver@1101.1.9
  - @pnpm/error@1100.0.1
  - @pnpm/installing.client@1100.2.10
  - @pnpm/fetching.directory-fetcher@1100.0.18
  - @pnpm/lockfile.types@1100.0.12
  - @pnpm/exec.lifecycle@1100.1.1
  - @pnpm/fs.indexed-pkg-importer@1100.0.16
  - @pnpm/lockfile.fs@1100.1.7
  - @pnpm/engine.runtime.commands@1100.1.7
  - @pnpm/cli.utils@1101.0.13
  - @pnpm/config.reader@1101.10.1
  - @pnpm/releasing.exportable-manifest@1100.1.8
  - @pnpm/network.auth-header@1101.1.3
  - @pnpm/network.web-auth@1101.1.2

## 1100.5.1

### Patch Changes

- e85aea2: Avoid reading `README.md` from disk when publishing if the publish manifest already provides a `readme` field. The README is now only read lazily, inside `createExportableManifest`, when it is actually needed.
- 9d0a300: Fixed `pnpm version --recursive` so it honors the workspace selection. In recursive mode the version bump now applies to the packages resolved from the workspace filter (`selectedProjectsGraph`), matching the behavior of `pnpm publish --recursive`, instead of always bumping every workspace package [#11348](https://github.com/pnpm/pnpm/issues/11348).
- Updated dependencies [302a2f7]
- Updated dependencies [c112b61]
- Updated dependencies [1b02b47]
- Updated dependencies [61969fb]
- Updated dependencies [9d79ba1]
- Updated dependencies [0474a9c]
- Updated dependencies [223d060]
- Updated dependencies [e85aea2]
- Updated dependencies [0474a9c]
- Updated dependencies [6d35338]
- Updated dependencies [4ca9247]
- Updated dependencies [eba03e0]
  - @pnpm/config.reader@1101.10.0
  - @pnpm/installing.commands@1100.10.0
  - @pnpm/fs.indexed-pkg-importer@1100.0.15
  - @pnpm/lockfile.fs@1100.1.6
  - @pnpm/network.git-utils@1100.0.2
  - @pnpm/releasing.exportable-manifest@1100.1.7
  - @pnpm/exec.lifecycle@1100.1.0
  - @pnpm/engine.runtime.node-resolver@1101.1.8
  - @pnpm/installing.client@1100.2.9
  - @pnpm/engine.runtime.commands@1100.1.6

## 1100.5.0

### Minor Changes

- f1521cf: Added a new opt-in `--batch` flag to `pnpm publish --recursive` that sends all selected packages to the registry in a single `PUT /-/pnpm/v1/publish` request instead of one request per package. The target registry has to implement the batch publish endpoint (pnpr does); registries that don't are reported with a clear `ERR_PNPM_BATCH_PUBLISH_UNSUPPORTED` error. The batch is processed all-or-nothing by pnpr: if any package in the batch fails validation, none of the packages are published.

### Patch Changes

- 7cdf9f8: Fixed `pnpm publish` ignoring `strictSsl: false` when publishing to registries with self-signed certificates. The `strictSSL` option is now forwarded to `libnpmpublish` / `npm-registry-fetch` so that `strict-ssl=false` in `.npmrc` or `strictSsl: false` in `pnpm-workspace.yaml` is respected during publish, the same way it is for `pnpm install` [pnpm/pnpm#12012](https://github.com/pnpm/pnpm/issues/12012).
- 681b593: pnpm can now use different auth tokens for different package scopes, even when those scopes use the same registry URL.

  Previously, auth was selected only by registry URL. If `@org-a` and `@org-b` both used `https://npm.pkg.github.com/`, they had to share the same token. This caused problems for registries that issue tokens per organization or per scope.

  Configure a scope-specific token by adding the package scope after the registry URL in the auth key:

  ```ini
  @org-a:registry=https://npm.pkg.github.com/
  @org-b:registry=https://npm.pkg.github.com/

  //npm.pkg.github.com/:@org-a:_authToken=${ORG_A_TOKEN}
  //npm.pkg.github.com/:@org-b:_authToken=${ORG_B_TOKEN}

  //npm.pkg.github.com/:_authToken=${FALLBACK_TOKEN}
  ```

  `pnpm login --registry=https://npm.pkg.github.com --scope=@org-a` writes the token to the same scope-specific auth key.

  When installing or publishing `@org-a/*`, pnpm uses `ORG_A_TOKEN`. For `@org-b/*`, pnpm uses `ORG_B_TOKEN`. Packages without a matching scope continue to use the registry-wide fallback token.

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [8dcd9a0]
- Updated dependencies [86e70d2]
- Updated dependencies [61810aa]
- Updated dependencies [ab0b7d1]
- Updated dependencies [74a2dc9]
- Updated dependencies [23716ed]
- Updated dependencies [681b593]
- Updated dependencies [d50d691]
- Updated dependencies [a31faa7]
  - @pnpm/installing.commands@1100.9.0
  - @pnpm/config.reader@1101.9.0
  - @pnpm/exec.lifecycle@1100.0.18
  - @pnpm/network.auth-header@1101.1.2
  - @pnpm/types@1101.3.2
  - @pnpm/lockfile.fs@1100.1.5
  - @pnpm/cli.utils@1101.0.12
  - @pnpm/deps.path@1100.0.8
  - @pnpm/engine.runtime.commands@1100.1.5
  - @pnpm/engine.runtime.node-resolver@1101.1.7
  - @pnpm/fetching.directory-fetcher@1100.0.17
  - @pnpm/fs.indexed-pkg-importer@1100.0.14
  - @pnpm/network.fetch@1100.1.3
  - @pnpm/network.web-auth@1101.1.1
  - @pnpm/installing.client@1100.2.8
  - @pnpm/bins.resolver@1100.0.8
  - @pnpm/config.pick-registry-for-package@1100.0.9
  - @pnpm/lockfile.types@1100.0.11
  - @pnpm/releasing.exportable-manifest@1100.1.6
  - @pnpm/resolving.resolver-base@1100.4.2
  - @pnpm/workspace.projects-filter@1100.0.21
  - @pnpm/workspace.projects-sorter@1100.0.7

## 1100.4.4

### Patch Changes

- Updated dependencies [bc9ed78]
- Updated dependencies [d976edf]
- Updated dependencies [615c669]
  - @pnpm/config.reader@1101.8.0
  - @pnpm/installing.commands@1100.8.0
  - @pnpm/engine.runtime.commands@1100.1.4
  - @pnpm/engine.runtime.node-resolver@1101.1.6
  - @pnpm/exec.lifecycle@1100.0.17
  - @pnpm/fs.indexed-pkg-importer@1100.0.13
  - @pnpm/network.fetch@1100.1.2
  - @pnpm/cli.utils@1101.0.11
  - @pnpm/installing.client@1100.2.7
  - @pnpm/releasing.exportable-manifest@1100.1.5
  - @pnpm/fetching.directory-fetcher@1100.0.16
  - @pnpm/workspace.projects-filter@1100.0.20

## 1100.4.3

### Patch Changes

- 65443f4: Reject invalid package names and versions from staged tarball manifests before deriving filenames for `pnpm stage download`.
- Updated dependencies [822beb5]
- Updated dependencies [3537020]
- Updated dependencies [894ea6a]
- Updated dependencies [6b5d91a]
- Updated dependencies [027196b]
- Updated dependencies [5f2bb9f]
- Updated dependencies [1017c36]
- Updated dependencies [e4d2fe0]
- Updated dependencies [230df57]
- Updated dependencies [bf1b731]
- Updated dependencies [3d50680]
  - @pnpm/config.reader@1101.7.0
  - @pnpm/installing.commands@1100.7.3
  - @pnpm/cli.common-cli-options-help@1100.0.2
  - @pnpm/bins.resolver@1100.0.7
  - @pnpm/types@1101.3.1
  - @pnpm/engine.runtime.node-resolver@1101.1.5
  - @pnpm/engine.runtime.commands@1100.1.3
  - @pnpm/releasing.exportable-manifest@1100.1.4
  - @pnpm/installing.client@1100.2.6
  - @pnpm/cli.utils@1101.0.10
  - @pnpm/config.pick-registry-for-package@1100.0.8
  - @pnpm/deps.path@1100.0.7
  - @pnpm/exec.lifecycle@1100.0.16
  - @pnpm/fetching.directory-fetcher@1100.0.15
  - @pnpm/lockfile.fs@1100.1.4
  - @pnpm/lockfile.types@1100.0.10
  - @pnpm/network.auth-header@1101.1.1
  - @pnpm/network.fetch@1100.1.1
  - @pnpm/resolving.resolver-base@1100.4.1
  - @pnpm/workspace.projects-filter@1100.0.19
  - @pnpm/workspace.projects-sorter@1100.0.6
  - @pnpm/fs.indexed-pkg-importer@1100.0.12

## 1100.4.2

### Patch Changes

- Updated dependencies [60a1eec]
- Updated dependencies [cbfeeef]
- Updated dependencies [5192edf]
- Updated dependencies [a017bf3]
- Updated dependencies [6d17b66]
  - @pnpm/network.fetch@1100.1.0
  - @pnpm/fs.indexed-pkg-importer@1100.0.11
  - @pnpm/network.auth-header@1101.1.0
  - @pnpm/config.reader@1101.6.0
  - @pnpm/types@1101.3.0
  - @pnpm/installing.commands@1100.7.2
  - @pnpm/resolving.resolver-base@1100.4.0
  - @pnpm/engine.runtime.commands@1100.1.2
  - @pnpm/engine.runtime.node-resolver@1101.1.4
  - @pnpm/installing.client@1100.2.5
  - @pnpm/bins.resolver@1100.0.6
  - @pnpm/cli.utils@1101.0.9
  - @pnpm/config.pick-registry-for-package@1100.0.7
  - @pnpm/deps.path@1100.0.6
  - @pnpm/exec.lifecycle@1100.0.15
  - @pnpm/fetching.directory-fetcher@1100.0.14
  - @pnpm/lockfile.fs@1100.1.3
  - @pnpm/lockfile.types@1100.0.9
  - @pnpm/releasing.exportable-manifest@1100.1.3
  - @pnpm/workspace.projects-filter@1100.0.18
  - @pnpm/workspace.projects-sorter@1100.0.5

## 1100.4.1

### Patch Changes

- Updated dependencies [33921c8]
  - @pnpm/releasing.exportable-manifest@1100.1.2
  - @pnpm/installing.commands@1100.7.1

## 1100.4.0

### Minor Changes

- 2cadfb5: Replaced `enquirer` with `@inquirer/prompts` for all interactive prompts. Fixes the `update -i` scrolling overflow bug where long choice lists were clipped in the terminal [#6643](https://github.com/pnpm/pnpm/issues/6643).

  **User-facing changes:**

  - `pnpm update -i` / `pnpm update -i --latest`: Scrolling now works correctly when many packages are available; the new library uses visual-line-aware pagination via `usePagination`
  - `pnpm audit --fix -i`: Same scrolling fix for vulnerability selection
  - `pnpm approve-builds`: Interactive build approval prompts updated
  - `pnpm patch`: Version selection and "apply to all" prompts updated
  - `pnpm patch-remove`: Patch removal selection updated
  - `pnpm publish`: Branch confirmation prompt updated
  - `pnpm login`: Credential prompts updated
  - `pnpm run` / `pnpm exec` (with `verifyDepsBeforeRun=prompt`): Confirmation prompt updated

  Vim-style `j`/`k` keys still work for up/down navigation in all interactive prompts.

  **Internal:** The `OtpEnquirer` and `LoginEnquirer` DI interfaces changed from `{ prompt }` to `{ input }` / `{ input, password }` respectively. Plugins or custom builds that inject their own enquirer mock will need to update.

### Patch Changes

- c94b4f8: Fix scoped packages without a publishConfig.access setting being published with public access.
- Updated dependencies [b1fa2d5]
- Updated dependencies [a39a83d]
- Updated dependencies [2cadfb5]
  - @pnpm/network.fetch@1100.0.8
  - @pnpm/config.reader@1101.5.0
  - @pnpm/installing.commands@1100.7.0
  - @pnpm/network.web-auth@1101.1.0
  - @pnpm/engine.runtime.commands@1100.1.1
  - @pnpm/engine.runtime.node-resolver@1101.1.3
  - @pnpm/installing.client@1100.2.4
  - @pnpm/workspace.projects-filter@1100.0.17
  - @pnpm/exec.lifecycle@1100.0.14
  - @pnpm/fs.indexed-pkg-importer@1100.0.10
  - @pnpm/releasing.exportable-manifest@1100.1.1

## 1100.3.1

### Patch Changes

- 6316e7b: Fix `pnpm deploy` crashing with `ENOENT: ... lstat '<deployDir>/node_modules'` when `configDependencies` declares pacquet (`pacquet` or `@pnpm/pacquet`). The deploy directory never installs config dependencies, so the install engine they designate isn't on disk to invoke; the nested install now skips them.
- Updated dependencies [a23956e]
- Updated dependencies [aa6149d]
- Updated dependencies [572842a]
- Updated dependencies [35d2355]
- Updated dependencies [a662de4]
  - @pnpm/config.reader@1101.4.1
  - @pnpm/network.auth-header@1101.0.0
  - @pnpm/installing.commands@1100.6.0
  - @pnpm/types@1101.2.0
  - @pnpm/engine.runtime.commands@1100.1.0
  - @pnpm/workspace.projects-filter@1100.0.16
  - @pnpm/engine.runtime.node-resolver@1101.1.2
  - @pnpm/installing.client@1100.2.3
  - @pnpm/cli.utils@1101.0.8
  - @pnpm/fetching.directory-fetcher@1100.0.13
  - @pnpm/releasing.exportable-manifest@1100.1.1
  - @pnpm/lockfile.fs@1100.1.2
  - @pnpm/bins.resolver@1100.0.5
  - @pnpm/config.pick-registry-for-package@1100.0.6
  - @pnpm/deps.path@1100.0.5
  - @pnpm/exec.lifecycle@1100.0.14
  - @pnpm/lockfile.types@1100.0.8
  - @pnpm/network.fetch@1100.0.7
  - @pnpm/resolving.resolver-base@1100.3.1
  - @pnpm/workspace.projects-sorter@1100.0.4
  - @pnpm/fs.indexed-pkg-importer@1100.0.10

## 1100.3.0

### Minor Changes

- 3b62f9d: Add a `skip-manifest-obfuscation` option for `pnpm pack` and `pnpm publish`. When enabled, the original `packageManager` field and publish lifecycle scripts are kept in the packed/published manifest instead of being stripped. The pnpm-specific `pnpm` field continues to be omitted.
- 508e6d8: Added `pnpm stage` with `publish`, `list`, `view`, `approve`, `reject`, and `download` subcommands for npm staged publishing.

### Patch Changes

- Updated dependencies [3b62f9d]
- Updated dependencies [212315d]
  - @pnpm/releasing.exportable-manifest@1100.1.0
  - @pnpm/config.reader@1101.4.0
  - @pnpm/installing.commands@1100.5.0
  - @pnpm/cli.utils@1101.0.7
  - @pnpm/fetching.directory-fetcher@1100.0.12
  - @pnpm/engine.runtime.commands@1100.0.17
  - @pnpm/engine.runtime.node-resolver@1101.1.1
  - @pnpm/installing.client@1100.2.2
  - @pnpm/exec.lifecycle@1100.0.13
  - @pnpm/workspace.projects-filter@1100.0.15

## 1100.2.18

### Patch Changes

- Updated dependencies [881a865]
  - @pnpm/installing.commands@1100.4.2

## 1100.2.17

### Patch Changes

- Updated dependencies [097983f]
  - @pnpm/config.pick-registry-for-package@1100.0.5
  - @pnpm/installing.commands@1100.4.1
  - @pnpm/installing.client@1100.2.1
  - @pnpm/workspace.projects-filter@1100.0.14

## 1100.2.16

### Patch Changes

- 64afc92: Honor `publishConfig.access` when publishing packages.
- Updated dependencies [3687b0e]
- Updated dependencies [ced20cb]
- Updated dependencies [a620557]
- Updated dependencies [9cb48bb]
- Updated dependencies [d1b340f]
- Updated dependencies [1627943]
- Updated dependencies [b206a15]
- Updated dependencies [64afc92]
  - @pnpm/config.reader@1101.3.3
  - @pnpm/installing.commands@1100.4.0
  - @pnpm/lockfile.fs@1100.1.1
  - @pnpm/exec.lifecycle@1100.0.12
  - @pnpm/installing.client@1100.2.0
  - @pnpm/resolving.resolver-base@1100.3.0
  - @pnpm/engine.runtime.node-resolver@1101.1.0
  - @pnpm/types@1101.1.1
  - @pnpm/engine.runtime.commands@1100.0.16
  - @pnpm/fetching.directory-fetcher@1100.0.11
  - @pnpm/lockfile.types@1100.0.7
  - @pnpm/cli.utils@1101.0.6
  - @pnpm/bins.resolver@1100.0.4
  - @pnpm/config.pick-registry-for-package@1100.0.4
  - @pnpm/deps.path@1100.0.4
  - @pnpm/network.fetch@1100.0.6
  - @pnpm/releasing.exportable-manifest@1100.0.7
  - @pnpm/workspace.projects-filter@1100.0.13
  - @pnpm/workspace.projects-sorter@1100.0.3
  - @pnpm/fs.indexed-pkg-importer@1100.0.9

## 1100.2.15

### Patch Changes

- Updated dependencies [4195766]
- Updated dependencies [31538bf]
- Updated dependencies [020ac45]
- Updated dependencies [d3f8408]
- Updated dependencies [247d70b]
- Updated dependencies [6e93f35]
- Updated dependencies [a62f959]
- Updated dependencies [ba2c884]
- Updated dependencies [2a9bd89]
- Updated dependencies [8df408c]
  - @pnpm/resolving.resolver-base@1100.2.0
  - @pnpm/installing.client@1100.1.0
  - @pnpm/installing.commands@1100.3.0
  - @pnpm/config.reader@1101.3.2
  - @pnpm/exec.pnpm-cli-runner@1100.0.1
  - @pnpm/lockfile.fs@1100.1.0
  - @pnpm/engine.runtime.node-resolver@1101.0.9
  - @pnpm/fetching.directory-fetcher@1100.0.10
  - @pnpm/lockfile.types@1100.0.6
  - @pnpm/exec.lifecycle@1100.0.11
  - @pnpm/fs.indexed-pkg-importer@1100.0.8
  - @pnpm/engine.runtime.commands@1100.0.15
  - @pnpm/network.fetch@1100.0.5
  - @pnpm/workspace.projects-filter@1100.0.12
  - @pnpm/releasing.exportable-manifest@1100.0.6
  - @pnpm/cli.utils@1101.0.5

## 1100.2.14

### Patch Changes

- Updated dependencies [18a464f]
- Updated dependencies [180aee9]
  - @pnpm/network.fetch@1100.0.4
  - @pnpm/installing.commands@1100.2.2
  - @pnpm/lockfile.fs@1100.0.8
  - @pnpm/cli.utils@1101.0.4
  - @pnpm/config.reader@1101.3.1
  - @pnpm/engine.runtime.commands@1100.0.14
  - @pnpm/engine.runtime.node-resolver@1101.0.8
  - @pnpm/installing.client@1100.0.15
  - @pnpm/exec.lifecycle@1100.0.10
  - @pnpm/fs.indexed-pkg-importer@1100.0.7
  - @pnpm/fetching.directory-fetcher@1100.0.9
  - @pnpm/releasing.exportable-manifest@1100.0.5
  - @pnpm/workspace.projects-filter@1100.0.11

## 1100.2.13

### Patch Changes

- @pnpm/installing.commands@1100.2.1
- @pnpm/installing.client@1100.0.14
- @pnpm/exec.lifecycle@1100.0.9

## 1100.2.12

### Patch Changes

- 20e7aff: `pnpm publish` now honors the configured HTTP/HTTPS proxy (including `https_proxy`/`http_proxy`/`no_proxy` environment variables) when polling the registry's `doneUrl` during the web-based authentication flow. Previously the poll bypassed the proxy, causing the registry to respond `403` from a different source IP and the login to never complete [#11561](https://github.com/pnpm/pnpm/issues/11561).
- Updated dependencies [20e7aff]
- Updated dependencies [b61e268]
- Updated dependencies [e1e29c1]
- Updated dependencies [a575dd2]
  - @pnpm/network.fetch@1100.0.3
  - @pnpm/config.reader@1101.3.0
  - @pnpm/types@1101.1.0
  - @pnpm/installing.commands@1100.2.0
  - @pnpm/engine.runtime.commands@1100.0.13
  - @pnpm/engine.runtime.node-resolver@1101.0.7
  - @pnpm/installing.client@1100.0.13
  - @pnpm/bins.resolver@1100.0.3
  - @pnpm/cli.utils@1101.0.3
  - @pnpm/config.pick-registry-for-package@1100.0.3
  - @pnpm/deps.path@1100.0.3
  - @pnpm/exec.lifecycle@1100.0.8
  - @pnpm/fetching.directory-fetcher@1100.0.8
  - @pnpm/lockfile.fs@1100.0.7
  - @pnpm/lockfile.types@1100.0.5
  - @pnpm/releasing.exportable-manifest@1100.0.4
  - @pnpm/resolving.resolver-base@1100.1.3
  - @pnpm/workspace.projects-filter@1100.0.10
  - @pnpm/workspace.projects-sorter@1100.0.2
  - @pnpm/fs.indexed-pkg-importer@1100.0.6

## 1100.2.11

### Patch Changes

- 80ef69b: Fixed `pnpm publish --provenance` failing with a 422 from the registry when the package version contained semver build metadata (e.g. `1.0.0-canary.0+abc1234`). The `+<build>` segment is now stripped before packing so that the version embedded in the tarball, the metadata sent to the registry, and the sigstore provenance subject all agree [#11518](https://github.com/pnpm/pnpm/issues/11518).
- Updated dependencies [e9e876c]
- Updated dependencies [dd8d5d7]
- Updated dependencies [15e9e35]
  - @pnpm/config.reader@1101.2.2
  - @pnpm/fs.packlist@1100.0.1
  - @pnpm/installing.commands@1100.1.12
  - @pnpm/installing.client@1100.0.12
  - @pnpm/engine.runtime.commands@1100.0.12
  - @pnpm/engine.runtime.node-resolver@1101.0.6
  - @pnpm/fetching.directory-fetcher@1100.0.7
  - @pnpm/exec.lifecycle@1100.0.7
  - @pnpm/workspace.projects-filter@1100.0.9
  - @pnpm/fs.indexed-pkg-importer@1100.0.5
  - @pnpm/releasing.exportable-manifest@1100.0.3

## 1100.2.10

### Patch Changes

- ce474cc: Run `preversion`, `version`, and `postversion` lifecycle scripts for `pnpm version`.
  - @pnpm/lockfile.fs@1100.0.6
  - @pnpm/installing.client@1100.0.11
  - @pnpm/installing.commands@1100.1.11

## 1100.2.9

### Patch Changes

- 90e215f: Make trusted publishing (OIDC) take precedence over a configured static `_authToken` in `pnpm publish`, mirroring the npm CLI's behavior. When OIDC succeeds, the OIDC-derived token overrides any pre-configured `_authToken`; when OIDC is not applicable (no CI environment, exchange fails, registry has no trusted publisher configured), the static token is used as a fallback. This applies on every package during recursive publish, so each workspace package independently attempts trusted publishing.

  Additionally, the `NPM_ID_TOKEN` env var is now honored as a CI-agnostic injection point for an OIDC ID token. Previously OIDC was only attempted on GitHub Actions or GitLab; now any CI provider that exposes its own OIDC mechanism (e.g. CircleCI's `CIRCLE_OIDC_TOKEN_V2`, Buildkite, etc.) can forward its token via `NPM_ID_TOKEN` and trusted publishing will work without pnpm needing to recognize the provider explicitly.

- 5607279: Restore npm-CLI-compatible `--json` stdout output for `pnpm publish` ([#11476](https://github.com/pnpm/pnpm/issues/11476)). pnpm 11 reimplemented publish natively ([#10591](https://github.com/pnpm/pnpm/pull/10591)) and inadvertently dropped the per-package JSON object that pnpm 10 emitted transitively via the npm CLI, silently breaking downstream tooling — most notably `nx release publish`, which parses stdout JSON to confirm success ([nrwl/nx#35575](https://github.com/nrwl/nx/issues/35575)). On success, the output is now:

  - `pnpm publish --json` → single object `{ id, name, version, size, unpackedSize, shasum, integrity, filename, files, entryCount, bundled }`, mirroring `npm publish --json`.
  - `pnpm publish -r --json` → array of those objects, mirroring `pnpm pack --json`'s shape choice.
  - `pnpm publish -r --report-summary` → existing `pnpm-publish-summary.json` envelope `{ publishedPackages: [...] }` is preserved, but each entry is upgraded to the same per-package shape (additive — `name` and `version` are still present).

- Updated dependencies [27425d7]
- Updated dependencies [707a879]
  - @pnpm/lockfile.fs@1100.0.5
  - @pnpm/lockfile.types@1100.0.4
  - @pnpm/resolving.resolver-base@1100.1.2
  - @pnpm/config.reader@1101.2.1
  - @pnpm/installing.commands@1100.1.10
  - @pnpm/installing.client@1100.0.10
  - @pnpm/engine.runtime.node-resolver@1101.0.5
  - @pnpm/fetching.directory-fetcher@1100.0.6
  - @pnpm/engine.runtime.commands@1100.0.11
  - @pnpm/releasing.exportable-manifest@1100.0.3
  - @pnpm/exec.lifecycle@1100.0.6
  - @pnpm/fs.indexed-pkg-importer@1100.0.5
  - @pnpm/workspace.projects-filter@1100.0.8

## 1100.2.8

### Patch Changes

- Updated dependencies [8fdd9a9]
- Updated dependencies [5f34a8d]
- Updated dependencies [c969392]
- Updated dependencies [817b1b4]
- Updated dependencies [c969392]
- Updated dependencies [2de318b]
  - @pnpm/config.reader@1101.2.0
  - @pnpm/engine.runtime.commands@1100.0.10
  - @pnpm/engine.runtime.node-resolver@1101.0.4
  - @pnpm/installing.commands@1100.1.9
  - @pnpm/releasing.exportable-manifest@1100.0.3
  - @pnpm/installing.client@1100.0.9

## 1100.2.7

### Patch Changes

- 2b8932d: Fixed `pnpm publish` to honor `publishConfig.registry` from `package.json` when publishing a single package. The native publish flow introduced in v11 was reading the registry from `.npmrc` only, ignoring the per-package override [#11419](https://github.com/pnpm/pnpm/issues/11419).
- Updated dependencies [f6bc1db]
  - @pnpm/installing.commands@1100.1.8

## 1100.2.6

### Patch Changes

- Updated dependencies [42a8f29]
  - @pnpm/config.reader@1101.1.4
  - @pnpm/engine.runtime.commands@1100.0.9
  - @pnpm/engine.runtime.node-resolver@1101.0.3
  - @pnpm/installing.commands@1100.1.7
  - @pnpm/installing.client@1100.0.8

## 1100.2.5

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/config.pick-registry-for-package@1100.0.2
  - @pnpm/releasing.exportable-manifest@1100.0.3
  - @pnpm/cli.common-cli-options-help@1100.0.1
  - @pnpm/fetching.directory-fetcher@1100.0.5
  - @pnpm/workspace.projects-filter@1100.0.7
  - @pnpm/fs.indexed-pkg-importer@1100.0.4
  - @pnpm/resolving.resolver-base@1100.1.1
  - @pnpm/installing.commands@1100.1.6
  - @pnpm/installing.client@1100.0.7
  - @pnpm/network.git-utils@1100.0.1
  - @pnpm/exec.lifecycle@1100.0.5
  - @pnpm/bins.resolver@1100.0.2
  - @pnpm/config.reader@1101.1.3
  - @pnpm/network.fetch@1100.0.2
  - @pnpm/cli.utils@1101.0.2
  - @pnpm/deps.path@1100.0.2
  - @pnpm/engine.runtime.node-resolver@1101.0.2
  - @pnpm/lockfile.types@1100.0.3
  - @pnpm/lockfile.fs@1100.0.4
  - @pnpm/engine.runtime.commands@1100.0.8

## 1100.2.4

### Patch Changes

- 8c41c5c: Fix recursive publish summaries to report the manifest from `publishConfig.directory` when packages publish from a generated directory [#11239](https://github.com/pnpm/pnpm/issues/11239).
  - @pnpm/cli.utils@1101.0.1
  - @pnpm/installing.commands@1100.1.5
  - @pnpm/engine.runtime.commands@1100.0.7
  - @pnpm/workspace.projects-filter@1100.0.6

## 1100.2.3

### Patch Changes

- Updated dependencies [0fbcf74]
  - @pnpm/config.reader@1101.1.2
  - @pnpm/workspace.projects-filter@1100.0.5
  - @pnpm/engine.runtime.commands@1100.0.6
  - @pnpm/engine.runtime.node-resolver@1101.0.1
  - @pnpm/installing.commands@1100.1.4
  - @pnpm/installing.client@1100.0.6

## 1100.2.2

### Patch Changes

- Updated dependencies [9b23098]
  - @pnpm/engine.runtime.commands@1100.0.5
  - @pnpm/installing.client@1100.0.5
  - @pnpm/installing.commands@1100.1.3

## 1100.2.1

### Patch Changes

- eb7e6ae: Fix the `@pnpm/exe` SEA executable crashing at startup on Node.js v25.7+. Two separate regressions in `@pnpm/exe@11.0.0-rc.4` are addressed:

  1. `pnpm pack-app` now pins the Node.js used to write the SEA blob to the exact embedded runtime version. The SEA blob format changed in Node.js v25.7 (ESM entry-point support added a `ModuleFormat` header byte), so a blob produced by a pre-25.7 builder cannot be deserialized by a 25.7+ runtime and vice versa. In rc.4 the CI host Node.js (v25.6.1) built blobs embedded in a v25.9.0 runtime, tripping `SeaDeserializer::Read() ... format_value <= kModule` on every invocation. `pack-app` now downloads a host-arch builder Node.js of the target version when the running Node.js doesn't already match.

  2. The pnpm CJS SEA entry shim now loads `dist/pnpm.mjs` through `Module.createRequire(process.execPath)` instead of `await import(pathToFileURL(...).href)`. In Node.js v25.7+, the ambient `require` and `import()` inside a CJS SEA entry are replaced with embedder hooks that only resolve built-in module names, causing external `file://` loads to fail with `ERR_UNKNOWN_BUILTIN_MODULE`. An explicit `createRequire()` bypasses those hooks.

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
