---
"@pnpm/tools.plugin-commands-self-updater": patch
"@pnpm/default-reporter": patch
"@pnpm/cli-meta": patch
"pnpm": patch
---

pnpm no longer tells you to update itself with Corepack or with `pnpm add -g`:

* The update notification now suggests `pnpm self-update`, or the [standalone install script](https://pnpm.io/installation) when pnpm is running under Corepack. It used to suggest `corepack use pnpm@<version>`, or `pnpm add -g pnpm` / `pnpm add -g @pnpm/exe` when pnpm was not installed by the standalone script — but `pnpm add -g` refuses to install pnpm and points at `pnpm self-update` anyway, and `@pnpm/exe` is not published for pnpm v12 or newer, where the unscoped `pnpm` package is itself the native executable.
* `pnpm self-update` under Corepack now points at the standalone install script too, instead of telling you to update pnpm with Corepack.
