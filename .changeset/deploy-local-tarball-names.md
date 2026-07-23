---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm deploy` with a shared lockfile so local `file:` tarball dependencies keep their package name in the generated deploy lockfile. This prevents warm-store deploys from failing with `ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE` when the tarball filename includes the version.
