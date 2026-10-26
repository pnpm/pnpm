## 1101.2.8

### Patch Changes

- Fixed `pnpm install` failing with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a workspace package injected through a relative path, such as `workspace:../lib`, has `catalog:` dependencies. Their catalog entries now resolve the way they do for a package injected by name [#17280](https://github.com/pnpm/pnpm/issues/17280).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.8
  - @pnpm/resolving.resolver-base@1101.3.4
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.project-manifest-reader@1100.1.5
