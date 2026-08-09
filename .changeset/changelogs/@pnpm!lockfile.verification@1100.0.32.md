## 1100.0.32

### Patch Changes

- Speed up installs after adding `ignoredOptionalDependencies` patterns by removing newly ignored optional dependencies and pruning packages that are no longer reachable without resolving the dependency graph again.

- Updated dependencies:
  - @pnpm/installing.context@1100.1.1
