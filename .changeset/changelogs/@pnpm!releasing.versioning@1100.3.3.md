## 1100.3.3

### Patch Changes

- `pnpm change check` now validates the pending change intents in `.changeset/`. It fails when an intent names a package that is not in the workspace or cannot be released.

- `pnpm version` now applies pending bumps to private workspace packages. A private package's changelog is written to its committed `CHANGELOG.md`, also when `versioning.changelog.storage` is `registry`
  [pnpm/pnpm#13736](https://github.com/pnpm/pnpm/issues/13736),
  [pnpm/pnpm#13519](https://github.com/pnpm/pnpm/issues/13519).

- Updated dependencies:
  - @pnpm/error@1100.2.0
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
