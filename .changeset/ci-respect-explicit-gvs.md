---
"@pnpm/config": patch
"pnpm": patch
---

CI no longer force-disables `enableGlobalVirtualStore` when it was explicitly set by the user. Previously, `ci: true` (auto-detected or configured) would unconditionally set `enableGlobalVirtualStore` to `false`, even when the user had explicitly enabled it in `pnpm-workspace.yaml` or via CLI. Now, only the default value is overridden in CI — explicit user configuration is respected.
