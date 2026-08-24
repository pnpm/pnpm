---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

Fixed pnpm v11 incorrectly reporting `confirmModulesPurge` as unrecognized when set in `pnpm-workspace.yaml`. The Rust CLI now identifies the unsupported option as a pnpm v11 setting instead of suggesting an unrelated setting.
