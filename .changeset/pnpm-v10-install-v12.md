---
"@pnpm/tools.plugin-commands-self-updater": patch
"pnpm": patch
---

Fixed switching to and self-updating to pnpm v12. pnpm v12 (the Rust port) ships as the `pnpm` and `@pnpm/exe` npm packages whose bins are placeholders replaced at install time by the host's native binary from a `@pnpm/exe.<platform>-<arch>[-musl]` optional dependency. Because pnpm installs its own engine with `--ignore-scripts`, that relinking never ran, leaving a non-executable placeholder. pnpm now relinks the native binary itself for v12 (recognizing the new platform-package naming scheme and the native `pnpm` package), and verifies the native binary's npm registry signature before running it.
