---
"@pnpm/default-reporter": patch
"pnpm": patch
---

The update notification printed by the standalone pnpm build now suggests installing the `pnpm` package when the available update is pnpm v12 or newer. From v12 the unscoped `pnpm` package is itself the native executable and `@pnpm/exe` is no longer published alongside it, so the previous suggestion would have installed the newest v11 instead.
