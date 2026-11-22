## 1100.3.30

### Patch Changes

- When an install is delegated to the Rust engine, pnpm now renders the lockfile verification progress that the engine reports. The pass and failure lines show how many entries were checked.

- When an overrides entry requests a version that does not exist on the registry, `pnpm install` now names that override entry in the error. If the entry's selector has a version range, such as `foo@^1`, the error also shows the latest published version in that range.

- Internal type-safety cleanup with no change in behavior.

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.6
  - @pnpm/core-loggers@1101.0.6
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.19
  - @pnpm/error@1100.2.3
  - @pnpm/types@1102.1.5
