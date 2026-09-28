## 1101.0.7

### Patch Changes

- `pnpm config set --location=project` and `pnpm config delete --location=project`, run from a package inside a workspace, now write settings that belong in `pnpm-workspace.yaml` to the workspace root's `pnpm-workspace.yaml`. Before, they created a new `pnpm-workspace.yaml` in the current package, which made that package the workspace root. Settings stored in `.npmrc` are still written to the current directory [#13757](https://github.com/pnpm/pnpm/issues/13757).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/error@1100.2.1
  - @pnpm/object.property-path@1100.1.8
  - @pnpm/workspace.workspace-manifest-writer@1100.2.3
