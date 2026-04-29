# @pnpm/network.web-auth

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
