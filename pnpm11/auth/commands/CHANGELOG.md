# @pnpm/auth.commands

## 1100.2.6

### Patch Changes

- Updated dependencies [05b95ab]
- Updated dependencies [852d537]
  - @pnpm/network.fetch@1100.1.4
  - @pnpm/error@1100.0.1
  - @pnpm/registry-access.client@1100.1.5
  - @pnpm/cli.utils@1101.0.13
  - @pnpm/config.reader@1101.10.1
  - @pnpm/network.web-auth@1101.1.2

## 1100.2.5

### Patch Changes

- Updated dependencies [302a2f7]
- Updated dependencies [0474a9c]
  - @pnpm/config.reader@1101.10.0

## 1100.2.4

### Patch Changes

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

- Updated dependencies [61810aa]
- Updated dependencies [681b593]
- Updated dependencies [a31faa7]
  - @pnpm/config.reader@1101.9.0
  - @pnpm/cli.utils@1101.0.12
  - @pnpm/network.fetch@1100.1.3
  - @pnpm/network.web-auth@1101.1.1
  - @pnpm/registry-access.client@1100.1.4

## 1100.2.3

### Patch Changes

- Updated dependencies [bc9ed78]
- Updated dependencies [615c669]
  - @pnpm/config.reader@1101.8.0
  - @pnpm/network.fetch@1100.1.2
  - @pnpm/cli.utils@1101.0.11
  - @pnpm/registry-access.client@1100.1.3

## 1100.2.2

### Patch Changes

- Updated dependencies [822beb5]
- Updated dependencies [3537020]
- Updated dependencies [894ea6a]
- Updated dependencies [6b5d91a]
- Updated dependencies [027196b]
- Updated dependencies [1017c36]
  - @pnpm/config.reader@1101.7.0
  - @pnpm/cli.utils@1101.0.10
  - @pnpm/network.fetch@1100.1.1
  - @pnpm/registry-access.client@1100.1.2

## 1100.2.1

### Patch Changes

- Updated dependencies [60a1eec]
- Updated dependencies [a017bf3]
  - @pnpm/network.fetch@1100.1.0
  - @pnpm/config.reader@1101.6.0
  - @pnpm/registry-access.client@1100.1.1
  - @pnpm/cli.utils@1101.0.9

## 1100.2.0

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

- Updated dependencies [b1fa2d5]
- Updated dependencies [a39a83d]
- Updated dependencies [2cadfb5]
  - @pnpm/registry-access.client@1100.1.0
  - @pnpm/network.fetch@1100.0.8
  - @pnpm/config.reader@1101.5.0
  - @pnpm/network.web-auth@1101.1.0

## 1100.1.2

### Patch Changes

- ae21758: Refactor the dist-tag-add and login (classic adduser) handlers to delegate their PUTs to a new shared package `@pnpm/registry-access.client`. Downstream tests in this monorepo now use these helpers (via `@pnpm/testing.registry-mock`) instead of `addDistTag` / `addUser` from `@pnpm/registry-mock`, which relied on the unmaintained `anonymous-npm-registry-client`.
- Updated dependencies [a23956e]
- Updated dependencies [35d2355]
  - @pnpm/config.reader@1101.4.1
  - @pnpm/cli.utils@1101.0.8
  - @pnpm/network.fetch@1100.0.7
  - @pnpm/registry-access.client@1100.0.1

## 1100.1.1

### Patch Changes

- Updated dependencies [3b62f9d]
- Updated dependencies [212315d]
  - @pnpm/config.reader@1101.4.0
  - @pnpm/cli.utils@1101.0.7

## 1100.1.0

### Minor Changes

- 56f3851: Implement the documented `pnpm login --scope <scope>` flag. The scope is normalized (a leading `@` is added if missing; blank values are ignored) and an `@<scope>:registry=<registry>` mapping is written to the pnpm auth file alongside the auth token. Subsequent installs of `@<scope>/*` packages then route to the chosen registry. Previously `pnpm login --scope foo` errored with `Unknown option: 'scope'` despite the flag being listed in the online documentation [#11716](https://github.com/pnpm/pnpm/issues/11716).

### Patch Changes

- Updated dependencies [3687b0e]
- Updated dependencies [ced20cb]
- Updated dependencies [d1b340f]
  - @pnpm/config.reader@1101.3.3
  - @pnpm/cli.utils@1101.0.6
  - @pnpm/network.fetch@1100.0.6

## 1100.0.14

### Patch Changes

- Updated dependencies [020ac45]
- Updated dependencies [d3f8408]
- Updated dependencies [a62f959]
- Updated dependencies [ba2c884]
- Updated dependencies [8df408c]
  - @pnpm/config.reader@1101.3.2
  - @pnpm/network.fetch@1100.0.5
  - @pnpm/cli.utils@1101.0.5

## 1100.0.13

### Patch Changes

- Updated dependencies [18a464f]
  - @pnpm/network.fetch@1100.0.4
  - @pnpm/cli.utils@1101.0.4
  - @pnpm/config.reader@1101.3.1

## 1100.0.12

### Patch Changes

- Updated dependencies [20e7aff]
- Updated dependencies [b61e268]
- Updated dependencies [e1e29c1]
  - @pnpm/network.fetch@1100.0.3
  - @pnpm/config.reader@1101.3.0
  - @pnpm/cli.utils@1101.0.3

## 1100.0.11

### Patch Changes

- Updated dependencies [e9e876c]
  - @pnpm/config.reader@1101.2.2

## 1100.0.10

### Patch Changes

- Updated dependencies [707a879]
  - @pnpm/config.reader@1101.2.1

## 1100.0.9

### Patch Changes

- Updated dependencies [8fdd9a9]
- Updated dependencies [5f34a8d]
- Updated dependencies [c969392]
- Updated dependencies [817b1b4]
- Updated dependencies [c969392]
- Updated dependencies [2de318b]
  - @pnpm/config.reader@1101.2.0

## 1100.0.8

### Patch Changes

- Updated dependencies [42a8f29]
  - @pnpm/config.reader@1101.1.4

## 1100.0.7

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/config.reader@1101.1.3
  - @pnpm/network.fetch@1100.0.2
  - @pnpm/cli.utils@1101.0.2

## 1100.0.6

### Patch Changes

- @pnpm/cli.utils@1101.0.1

## 1100.0.5

### Patch Changes

- Updated dependencies [0fbcf74]
  - @pnpm/config.reader@1101.1.2

## 1100.0.4

### Patch Changes

- @pnpm/config.reader@1101.1.1

## 1100.0.3

### Patch Changes

- Updated dependencies [7d25bc1]
- Updated dependencies [9e0833c]
  - @pnpm/config.reader@1101.1.0

## 1100.0.2

### Patch Changes

- Updated dependencies [cee550a]
- Updated dependencies [4ab3d9b]
- Updated dependencies [9af708a]
- Updated dependencies [ea2a7fb]
- Updated dependencies [ff7733c]
  - @pnpm/cli.utils@1101.0.0
  - @pnpm/config.reader@1101.0.0

## 1100.0.1

### Patch Changes

- Internally, `@pnpm/network.web-auth`'s `promptBrowserOpen` now uses the [`open`](https://www.npmjs.com/package/open) package instead of spawning platform-specific commands. The `execFile` field and `PromptBrowserOpenExecFile` / `PromptBrowserOpenProcess` type exports have been removed from `PromptBrowserOpenContext`.
- Updated dependencies
  - @pnpm/network.web-auth@1101.0.0
  - @pnpm/cli.utils@1100.0.1
  - @pnpm/config.reader@1100.0.1
  - @pnpm/network.fetch@1100.0.1

## 1000.1.0

### Minor Changes

- de3dc74: During web-based authentication (`pnpm login`, `pnpm publish`), users can now press ENTER to open the authentication URL in their default browser. The background polling continues uninterrupted, so users who prefer to authenticate on their phone can still do so without pressing anything.
- d4a1d73: Added `pnpm login` command for authenticating with npm registries. Supports web-based login (with QR code) and classic username/password login as a fallback. The `adduser` command is aliased to `login`.
- 16cfde6: Added `pnpm logout` command for logging out of npm registries. Revokes the authentication token on the registry and removes it from the local configuration.
- 2df8b71: pnpm no longer reads all settings from `.npmrc`. Only auth and registry settings are read from `.npmrc` files. All other settings (like `hoist-pattern`, `node-linker`, `shamefully-hoist`, etc.) must be configured in `pnpm-workspace.yaml` or the global `~/.config/pnpm/config.yaml`.

  ### What changed

  **`.npmrc` is now only for auth and registry settings.** pnpm-specific settings in `.npmrc` are ignored. Move them to `pnpm-workspace.yaml`.

  **pnpm no longer reads `npm_config_*` environment variables.** Use `pnpm_config_*` environment variables instead (e.g., `pnpm_config_registry` instead of `npm_config_registry`).

  **pnpm no longer reads the npm global config** at `$PREFIX/etc/npmrc`.

  **`pnpm login` writes auth tokens** to `~/.config/pnpm/auth.ini`.

  ### Settings still read from `.npmrc`

  The following settings continue to be read from `.npmrc` files (project-level and `~/.npmrc`):

  - `registry` and `@scope:registry` — registry URLs
  - `//registry.example.com/:_authToken` — auth tokens per registry
  - `_auth`, `_authToken`, `_password`, `username`, `email` — global auth credentials
  - `//registry.example.com/:tokenHelper` — token helper commands
  - `ca`, `cafile`, `cert`, `key`, `certfile`, `keyfile` — SSL certificates
  - `strict-ssl` — SSL verification
  - `proxy`, `https-proxy`, `no-proxy` — proxy settings
  - `local-address` — local network address binding
  - `git-shallow-hosts` — git shallow clone hosts

  ### New `npmrcAuthFile` setting

  A new `npmrcAuthFile` setting can be added to `pnpm-workspace.yaml` or `~/.config/pnpm/config.yaml` to specify a custom path to the user `.npmrc` file (defaults to `~/.npmrc`):

  ```yaml
  npmrcAuthFile: /custom/path/.npmrc
  ```

  ### New `registries` setting in `pnpm-workspace.yaml`

  Registry URLs can now be configured in `pnpm-workspace.yaml`, so there's no need to commit `.npmrc` files with registry mappings:

  ```yaml
  registries:
    default: https://registry.npmjs.org/
    "@my-org": https://private.example.com/
    "@internal": https://nexus.corp.com/
  ```

  This replaces the `.npmrc` settings `registry=...` and `@scope:registry=...`.

  ### Auth file read order (highest priority first)

  1. `~/.config/pnpm/auth.ini` — pnpm's own auth file (written by `pnpm login`)
  2. `<workspace>/.npmrc` — workspace root (or project root)
  3. `~/.npmrc` (or custom `npmrcAuthFile`) — user-level fallback

  Note: `.npmrc` is only read from the workspace root, not from individual package directories.

  ### Migration guide

  1. **Move pnpm settings from `.npmrc` to `pnpm-workspace.yaml`:**

     Before (`.npmrc`):

     ```ini
     shamefully-hoist=true
     node-linker=hoisted
     ```

     After (`pnpm-workspace.yaml`):

     ```yaml
     shamefullyHoist: true
     nodeLinker: hoisted
     ```

  2. **Move scoped registry mappings from `.npmrc` to `pnpm-workspace.yaml`:**

     Before (`.npmrc`):

     ```ini
     @my-org:registry=https://private.example.com
     ```

     After (`pnpm-workspace.yaml`):

     ```yaml
     registries:
       "@my-org": https://private.example.com/
     ```

  3. **If you use `npm_config_*` env vars**, switch to `pnpm_config_*`:

     ```sh
     # Before
     npm_config_registry=https://registry.example.com

     # After
     pnpm_config_registry=https://registry.example.com
     ```

  4. **Auth tokens in `~/.npmrc` still work.** No migration needed for registry authentication — pnpm continues to read `~/.npmrc` as a fallback.

### Patch Changes

- Updated dependencies [7730a7f]
- Updated dependencies [ae8b816]
- Updated dependencies [facdd71]
- Updated dependencies [de3dc74]
- Updated dependencies [3c72b6b]
- Updated dependencies [9f5c0e3]
- Updated dependencies [76718b3]
- Updated dependencies [cc1b8e3]
- Updated dependencies [90bd3c3]
- Updated dependencies [1cc61e8]
- Updated dependencies [606f53e]
- Updated dependencies [c7203b9]
- Updated dependencies [bb17724]
- Updated dependencies [da2429d]
- Updated dependencies [1cc61e8]
- Updated dependencies [d4a1d73]
- Updated dependencies [491a84f]
- Updated dependencies [f0ae1b9]
- Updated dependencies [0dfa8b8]
- Updated dependencies [7fab2a2]
- Updated dependencies [cb367b9]
- Updated dependencies [543c7e4]
- Updated dependencies [075aa99]
- Updated dependencies [ae43ac7]
- Updated dependencies [ccec8e7]
- Updated dependencies [4158906]
- Updated dependencies [ac944ef]
- Updated dependencies [7d2fd48]
- Updated dependencies [cc7c0d2]
- Updated dependencies [d5d4eed]
- Updated dependencies [095f659]
- Updated dependencies [96704a1]
- Updated dependencies [bb8baa7]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [51b04c3]
- Updated dependencies [6c480a4]
- Updated dependencies [d01b81f]
- Updated dependencies [3ed41f4]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [ed1a7fe]
- Updated dependencies [cc7c0d2]
- Updated dependencies [5bf7768]
- Updated dependencies [ae43ac7]
- Updated dependencies [a5fdbf9]
- Updated dependencies [9d3f00b]
- Updated dependencies [6b3d87a]
- Updated dependencies [9587dac]
- Updated dependencies [09a999a]
- Updated dependencies [559f903]
- Updated dependencies [3574905]
  - @pnpm/config.reader@1005.0.0
  - @pnpm/network.web-auth@1001.0.0
  - @pnpm/cli.utils@1002.0.0
  - @pnpm/error@1001.0.0
  - @pnpm/network.fetch@1001.0.0
