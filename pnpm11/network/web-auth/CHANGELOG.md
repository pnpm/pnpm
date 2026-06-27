# @pnpm/network.web-auth

## 1101.1.2

### Patch Changes

- Updated dependencies [852d537]
  - @pnpm/error@1100.0.1

## 1101.1.1

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

## 1101.1.0

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

## 1101.0.0

### Major Changes

- Internally, `@pnpm/network.web-auth`'s `promptBrowserOpen` now uses the [`open`](https://www.npmjs.com/package/open) package instead of spawning platform-specific commands. The `execFile` field and `PromptBrowserOpenExecFile` / `PromptBrowserOpenProcess` type exports have been removed from `PromptBrowserOpenContext`.

## 1001.0.0

### Major Changes

- d4a1d73: Create `@pnpm/network.web-auth`.

### Minor Changes

- de3dc74: During web-based authentication (`pnpm login`, `pnpm publish`), users can now press ENTER to open the authentication URL in their default browser. The background polling continues uninterrupted, so users who prefer to authenticate on their phone can still do so without pressing anything.

### Patch Changes

- Updated dependencies [491a84f]
- Updated dependencies [7d2fd48]
- Updated dependencies [831f574]
  - @pnpm/error@1001.0.0
