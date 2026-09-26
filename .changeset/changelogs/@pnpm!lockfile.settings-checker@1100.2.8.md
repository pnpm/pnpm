## 1100.2.8

### Patch Changes

- `pnpm install --ignore-pnpmfile` no longer removes `pnpmfileChecksum` from an up-to-date `pnpm-lock.yaml`. `pnpm install --frozen-lockfile --ignore-pnpmfile` no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when the lockfile records a `pnpmfileChecksum`. A command that resolves dependencies with the pnpmfile ignored still writes the lockfile without it [#10944](https://github.com/pnpm/pnpm/issues/10944).

- Updated dependencies:
  - @pnpm/config.parse-overrides@1100.1.6
  - @pnpm/crypto.hash@1100.0.6
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.verification@1100.1.7
