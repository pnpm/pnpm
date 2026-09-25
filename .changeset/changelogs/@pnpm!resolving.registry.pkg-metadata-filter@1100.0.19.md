## 1100.0.19

### Patch Changes

- With `trustPolicy: no-downgrade`, pnpm now resolves the newest matching version that is not a trust downgrade. Previously a dependency failed with `ERR_PNPM_TRUST_DOWNGRADE` even when an older version satisfied its range. `pnpm self-update` picks its target version the same way. A request for an exact version still fails [#14176](https://github.com/pnpm/pnpm/issues/14176).

- Updated dependencies:
  - @pnpm/resolving.registry.types@1100.2.1
