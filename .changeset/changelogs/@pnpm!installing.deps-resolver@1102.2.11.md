## 1102.2.11

### Patch Changes

- The error for a dependency with an invalid name in a project's manifest now names the project's directory. It used to say only "The current package", which did not tell which workspace project to fix [#17475](https://github.com/pnpm/pnpm/issues/17475).

- `pnpm add` no longer rewrites a catalog entry set to the dist-tag that the dependency resolves through. For example, an entry `beta` stays `beta` after `pnpm add foo@beta` or `pnpm add foo`. Before, the entry was replaced with the version the tag pointed to [pnpm/tasks#225](https://github.com/pnpm/tasks/issues/225).

- Updated dependencies:
  - @pnpm/deps.graph-hasher@1100.3.10
  - @pnpm/lockfile.preferred-versions@1100.0.41
  - @pnpm/lockfile.utils@1102.1.9
