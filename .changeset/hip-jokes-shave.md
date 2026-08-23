---
"@pnpm/default-reporter": patch
"pnpm": patch
---

The update notification now suggests running `pnpm self-update` (or `corepack use` when pnpm runs under Corepack). It used to suggest `pnpm add -g pnpm` or `pnpm add -g @pnpm/exe` when pnpm was not installed by the standalone script, but `pnpm add -g` refuses to install pnpm and points at `pnpm self-update` anyway. `@pnpm/exe` was also the wrong package to name for an update to pnpm v12 or newer, where the unscoped `pnpm` package is itself the native executable and `@pnpm/exe` is no longer published.
