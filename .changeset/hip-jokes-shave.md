---
"@pnpm/default-reporter": patch
"pnpm": patch
---

The update notification printed by the standalone pnpm build now suggests installing the package that `pnpm self-update` would actually install: the `pnpm` package when the available update is pnpm v12 or newer (from v12 the unscoped `pnpm` package is itself the native executable and `@pnpm/exe` is no longer published alongside it), and when a v11 update is offered on Intel macOS, where `@pnpm/exe` ships no darwin-x64 binary [#11423](https://github.com/pnpm/pnpm/issues/11423). Previously both cases suggested `@pnpm/exe`, which would have installed a release without a working binary for the user's platform.
