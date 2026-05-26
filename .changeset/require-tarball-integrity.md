---
"@pnpm/lockfile.utils": patch
"pnpm": patch
---

Reject `pnpm-lock.yaml` entries whose tarball-shaped `resolution:` block is missing the `integrity` field. Previously the worker that extracts a downloaded tarball skipped hash verification when no integrity was supplied and minted a fresh one from the unverified bytes, so an attacker who could both alter the lockfile (e.g. via a pull request that strips `integrity:`) and serve modified content at the referenced tarball URL could install a tampered package without any error — including under `--frozen-lockfile`. pnpm now fails closed at lockfile-read time with `ERR_PNPM_MISSING_TARBALL_INTEGRITY`, matching pacquet's existing `pacquet_package_manager::missing_tarball_integrity` guard.
