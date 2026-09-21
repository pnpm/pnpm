## 1100.3.3

### Patch Changes

- pnpm now measures a `pnpm.overrides` entry written as a bare path, such as `./local-dep`, from the directory holding `pnpm-workspace.yaml`. It used to be measured from each package the override rewrote, so the dependency linked to a directory that does not exist [#11131](https://github.com/pnpm/pnpm/issues/11131).

- Updated dependencies:
  - @pnpm/resolving.local-resolver@1101.2.2
