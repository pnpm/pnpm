## 1100.1.30

### Patch Changes

- Git dependencies now accept npm's `::` separator between fragment parameters, such as `github:owner/repo#v1.0.0::path:packages/foo`. Before, pnpm read the whole fragment as a branch name and failed to resolve it [pnpm/pnpm#17827](https://github.com/orgs/pnpm/discussions/17827).

- Updated dependencies:
  - @pnpm/network.fetch@1100.1.25
  - @pnpm/resolving.resolver-base@1101.3.5
