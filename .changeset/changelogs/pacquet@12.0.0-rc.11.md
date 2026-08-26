## 12.0.0-rc.11

### Minor Changes

- `pnpm stage approve` now approves several staged packages at once. Run it without a stage id to pick from the staged versions interactively, or pass a list of stage ids. The whole batch is approved with a single one-time password, and pnpm asks for a new one only once the registry stops accepting it. Inside a workspace, the selected packages are approved in dependency order, and a package whose workspace dependency could not be approved is skipped instead of being published against a dependency that never reached the registry.

### Patch Changes

- Under `nodeLinker: isolated`, a Bit root-component member whose materialized copy carries no `package.json` now receives sibling symlinks for the dependencies its own lockfile snapshot declares, instead of a symlink to every other member of the root. The all-member fallback remains only when no snapshot exists.

- The update notification now suggests `pnpm self-update` when `PNPM_HOME` manages the pnpm in use, and the [standalone install script](https://pnpm.io/installation) otherwise — under Corepack, or when another package manager installed pnpm. `pnpm self-update` under Corepack names the standalone install script too.
