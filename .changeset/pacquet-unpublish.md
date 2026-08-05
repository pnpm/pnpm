---
"pacquet": minor
---

Added the `pnpm unpublish` command: remove a package from the registry entirely (requires `--force`), or remove the versions matching `<package>@<range>`, re-pointing `dist-tags` that referenced them and deleting the orphaned tarballs. Supports `--registry` and `--otp`.
