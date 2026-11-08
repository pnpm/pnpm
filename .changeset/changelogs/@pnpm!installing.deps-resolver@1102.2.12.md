## 1102.2.12

### Patch Changes

- Packages from the default registry keep plain lockfile keys, even when a prefix such as `npmjs` points at the registry set as `registry`. A dependency written with such a prefix shares the plain entry of the same package. The next install rewrites keys that earlier versions qualified this way [#17575](https://github.com/pnpm/pnpm/issues/17575).

- Fixed peer deduplication giving a project different copies of a dependency across direct and transitive paths when an optional peer has no provider [pnpm/pnpm#17666](https://github.com/pnpm/pnpm/issues/17666).

- Updated dependencies:
  - @pnpm/config.pick-registry-for-package@1101.0.5
  - @pnpm/deps.graph-hasher@1100.3.11
  - @pnpm/deps.graph-sequencer@1101.0.1
  - @pnpm/lockfile.preferred-versions@1100.0.42
  - @pnpm/lockfile.utils@1102.1.10
  - @pnpm/resolving.npm-resolver@1104.2.9
