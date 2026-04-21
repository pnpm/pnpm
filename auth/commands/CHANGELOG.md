# @pnpm/auth.commands

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
