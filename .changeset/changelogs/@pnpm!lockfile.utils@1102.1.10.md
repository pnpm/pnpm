## 1102.1.10

### Patch Changes

- Packages from the default registry keep plain lockfile keys, even when a prefix such as `npmjs` points at the registry set as `registry`. A dependency written with such a prefix shares the plain entry of the same package. The next install rewrites keys that earlier versions qualified this way [#17575](https://github.com/pnpm/pnpm/issues/17575).

- Updated dependencies:
  - @pnpm/config.pick-registry-for-package@1101.0.5
