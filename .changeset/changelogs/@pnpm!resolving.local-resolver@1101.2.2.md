## 1101.2.2

### Patch Changes

- pnpm now measures a `pnpm.overrides` entry written as a bare path, such as `./local-dep`, from the directory holding `pnpm-workspace.yaml`. It used to be measured from each package the override rewrote, so the dependency linked to a directory that does not exist [#11131](https://github.com/pnpm/pnpm/issues/11131).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.5
  - @pnpm/resolving.resolver-base@1101.3.0
  - @pnpm/workspace.project-manifest-reader@1100.0.29
