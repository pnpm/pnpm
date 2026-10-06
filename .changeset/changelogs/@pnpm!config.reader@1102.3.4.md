## 1102.3.4

### Patch Changes

- pnpm now prints config warnings, such as an unset environment variable in `.npmrc`, when loading the config fails.

- `pnpm run` and `pnpm exec` now forward `--config.*` command-line flags to the install started by `verifyDepsBeforeRun` [pnpm/tasks#60](https://github.com/pnpm/tasks/issues/60).

- `pnpm config get` and `pnpm config list` with `--global` or `--location=global` now show only the global configuration. Both flags included the project's `.npmrc` before. `--location=global` also included the project's `pnpm-workspace.yaml`. `pnpm config get --global` failed when the global bin directory was not in PATH [#16598](https://github.com/pnpm/pnpm/issues/16598).

- `pnpm dlx` now inherits the `release` entry of `nodeDownloadMirrors` from the workspace configuration when downloading Node.js runtimes [#11281](https://github.com/pnpm/pnpm/issues/11281). Other channels are not inherited: Node signs `SHASUMS256.txt` for `release` only, so a workspace-supplied `rc`/`nightly` mirror would provide both the archive and its checksum for a runtime that `dlx` executes. Mirrors the user configured globally for those channels continue to apply.

- The warnings about ignored project `.npmrc` registry and auth settings no longer print the username and password of a URL-scoped key such as `//user:password@registry.example.com/:_authToken`.

- Validate the httpProxy and httpsProxy settings as strings when configured in pnpm-workspace.yaml or global configuration.

- Updated dependencies:
  - @pnpm/catalogs.config@1100.0.10
  - @pnpm/constants@1102.0.1
  - @pnpm/error@1100.2.2
  - @pnpm/hooks.pnpmfile@1100.0.36
  - @pnpm/pkg-manifest.utils@1100.4.9
  - @pnpm/workspace.project-manifest-reader@1100.1.3
  - @pnpm/workspace.workspace-manifest-reader@1100.2.2
