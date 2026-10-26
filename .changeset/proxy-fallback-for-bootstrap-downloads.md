---
"@pnpm/config.reader": patch
"pnpm": patch
---

When `https-proxy` is `false`, pnpm now downloads the pnpm version pinned by `packageManager` through the legacy `proxy` setting, as installs already do [pnpm/tasks#193](https://github.com/pnpm/tasks/issues/193).
