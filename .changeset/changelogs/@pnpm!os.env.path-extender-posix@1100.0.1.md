## 1100.0.1

### Patch Changes

- `pnpm setup` now puts `$PNPM_HOME/bin` first on `PATH` in login shells that inherited it further down, such as the VS Code terminal on macOS. Before, another `node` took precedence over the one installed by `pnpm runtime set node -g`. Run `pnpm setup` again to update the block in your shell config [#16635](https://github.com/pnpm/pnpm/issues/16635).

- Updated dependencies:
  - @pnpm/error@1100.2.2
