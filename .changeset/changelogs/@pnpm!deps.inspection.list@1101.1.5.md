## 1101.1.5

### Patch Changes

- `pnpm list --parseable --long` now keeps `<name>@` on `link:` and `file:` dependencies whose path or peer suffix contains `@`, such as `link:../@scope/pkg`. Aliased scoped `file:` dependencies whose path contains `@` also keep `<name>@` in `pnpm list`.

- Updated dependencies:
  - @pnpm/deps.inspection.tree-builder@1101.1.5
  - @pnpm/lockfile.fs@1100.2.19
  - @pnpm/pkg-manifest.reader@1100.0.25
  - @pnpm/types@1102.1.4
  - @pnpm/workspace.project-manifest-reader@1100.1.7
