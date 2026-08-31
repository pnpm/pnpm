---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fixed `--side-effects-cache`/`--no-side-effects-cache` and `PNPM_CONFIG_SIDE_EFFECTS_CACHE` discarding a remote side-effects cache declared under the object form of `sideEffectsCache` in `pnpm-workspace.yaml`. The boolean now switches only the local cache off or on, as it already does when a config file declares it.
