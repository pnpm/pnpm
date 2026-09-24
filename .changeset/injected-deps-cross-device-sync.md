---
"pnpm": patch
"pacquet": patch
---

`pnpm run` now falls back to copying files when hard linking fails with a cross-device link error while syncing injected dependencies. A cross-device error previously caused the sync to abort with `ERR_PNPM_INJECTED_DEPS_SYNC_LINK` [pnpm/pnpm#14703](https://github.com/pnpm/pnpm/issues/14703).
