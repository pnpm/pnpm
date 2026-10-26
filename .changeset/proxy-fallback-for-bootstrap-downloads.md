---
"@pnpm/config.reader": patch
"pnpm": patch
---

When `https-proxy` is `false`, pnpm now downloads the pnpm version pinned by `packageManager` through the legacy `proxy` setting, as installs already do. Setting `proxy` to `false` now also stops that download from using the `HTTP_PROXY` environment variable. The signature check of the pinned pnpm now uses the same proxy and TLS settings as its download [pnpm/tasks#193](https://github.com/pnpm/tasks/issues/193).
