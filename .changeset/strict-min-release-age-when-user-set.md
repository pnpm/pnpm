---
"@pnpm/config.reader": patch
"pnpm": patch
---

`minimumReleaseAgeStrict` now defaults to `true` whenever the user explicitly sets `minimumReleaseAge` (via `pnpm-workspace.yaml`, the global `config.yaml`, the CLI, or `pnpm_config_*` env vars). Previously, an explicitly configured `minimumReleaseAge` could silently fall back to installing an immature version when no mature version satisfied the requested range, making the setting look like it had no effect [#11433](https://github.com/pnpm/pnpm/issues/11433). Set `minimumReleaseAgeStrict: false` to opt back into the silent fallback behavior. The built-in default (1440) remains non-strict, preserving the existing behavior for users who haven't configured the setting.
