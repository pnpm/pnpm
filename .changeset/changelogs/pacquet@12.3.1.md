## 12.3.1

### Patch Changes

- Sped up installs in large workspaces: the anchor for re-rendering workspace `link:` targets is now derived once per project instead of once per dependency edge, and project ordering hashes paths by their raw bytes [#14352](https://github.com/pnpm/pnpm/issues/14352).

- After a self-update from pnpm 12.2 to 12.3, global commands such as `node`, `npm`, and `yarn` failed with `unexpected argument '--shim' found`. Global commands now launch normally, and their first launch migrates the global bin directory to native shims. When self-update downgrades to pnpm 12.2 or older, it keeps the newer native shims so those commands continue to work.

- Sped up installs in large workspaces. The check that verifies each project against the lockfile now runs the projects in parallel [#14352](https://github.com/pnpm/pnpm/issues/14352).
