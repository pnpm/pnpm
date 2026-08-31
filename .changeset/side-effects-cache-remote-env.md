---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

The environment variables for the remote side-effects cache are named for the setting they configure: `PNPM_SIDE_EFFECTS_CACHE_REMOTE_KEY_ID`, `..._BUILDER_ID`, `..._IMAGE_DIGEST`, `..._ARCHITECTURE_BASELINE`, `..._PRIVATE_KEY`, `..._BUILD_ENV`, `..._TRUSTED_KEYS` and `..._PUBLISH`. The `PNPM_REMOTE_SIDE_EFFECTS_CACHE_*` names keep working, and the new one wins when both are set.
