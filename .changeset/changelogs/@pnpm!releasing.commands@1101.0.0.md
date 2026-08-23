## 1101.0.0

### Major Changes

- The three registry lookups are now named for what they are keyed by, so that none of them is called `registries` — a name the `registries` setting itself has taken:

  | before | after |
  |---|---|
  | `Config.registries` | `Config.registriesByScope` |
  | `Config.namedRegistries` | `Config.registriesByPrefix` |
  | `Config.registryOptions` | `Config.registryOptionsByUrl` |

  The same rename applies to the `RegistryContext` fields, the `Registries` and `NamedRegistries` types (now `RegistriesByScope` and `RegistriesByPrefix`), `normalizeRegistries` / `normalizeNamedRegistries` (now `normalizeRegistriesByScope` / `normalizeRegistriesByPrefix`), and the `BUILTIN_NAMED_REGISTRIES` constant (now `BUILTIN_REGISTRIES_BY_PREFIX`).

  This is an internal rename: no setting, error code, lockfile field, or `.pnpmfile.cjs` hook field changes. A `preResolution` hook still reads `ctx.registries`, which is the name pacquet passes as well. The `registries` and `namedRegistries` settings are read under the names users write them.

  The pnpr resolve request sends `registriesByPrefix` where it sent `namedRegistries`. A pnpr server and its clients must be on matching versions, which is already the case for an experimental server.

### Patch Changes

- The package and bump pickers of `pnpm change` now size their page from the terminal height instead of always showing 7 rows. They fall back to 7 rows when the terminal height is unknown [`pnpm/pnpm#13815`](https://github.com/pnpm/pnpm/issues/13815).

- Canceling a `pnpm change` prompt with Ctrl-c no longer prints a stack trace. It reports `Change canceled` and exits with a success status, like the other interactive commands [#13814](https://github.com/pnpm/pnpm/issues/13814).

- `pnpm deploy --prod` and `pnpm deploy --no-optional` no longer list the excluded dependency groups in the deployed `package.json` and `pnpm-lock.yaml`. The deployed lockfile referenced packages that the deploy left out of its graph, so installing in the deploy directory afterwards created dangling symlinks [#13623](https://github.com/pnpm/pnpm/issues/13623).

- Don't treat files like `license16.json` as a package license when deciding if the workspace LICENSE file should be included in the packed package.

- `pnpm version <bump>` with `--dry-run` no longer edits `package.json` files. It now only reports the bumps it would make, and skips the working tree check, the version lifecycle scripts, the commit, and the tag [`pnpm/pnpm#13953`](https://github.com/pnpm/pnpm/issues/13953).

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.15
  - @pnpm/cli.utils@1101.0.24
  - @pnpm/config.pick-registry-for-package@1101.0.0
  - @pnpm/config.reader@1102.0.0
  - @pnpm/constants@1102.0.0
  - @pnpm/deps.path@1101.0.0
  - @pnpm/engine.runtime.commands@1101.0.0
  - @pnpm/engine.runtime.node-resolver@1101.2.1
  - @pnpm/error@1100.1.3
  - @pnpm/exec.lifecycle@1100.1.14
  - @pnpm/fetching.directory-fetcher@1100.0.30
  - @pnpm/fs.indexed-pkg-importer@1100.0.26
  - @pnpm/installing.client@1100.3.5
  - @pnpm/installing.commands@1101.0.0
  - @pnpm/lockfile.fs@1100.2.3
  - @pnpm/lockfile.types@1100.0.20
  - @pnpm/network.auth-header@1101.1.11
  - @pnpm/network.fetch@1100.1.13
  - @pnpm/network.web-auth@1101.4.3
  - @pnpm/releasing.exportable-manifest@1100.2.4
  - @pnpm/releasing.versioning@1100.2.6
  - @pnpm/resolving.npm-resolver@1104.0.0
  - @pnpm/resolving.registry.types@1100.1.10
  - @pnpm/resolving.resolver-base@1101.1.1
  - @pnpm/types@1102.0.0
  - @pnpm/workspace.projects-filter@1100.0.38
  - @pnpm/workspace.projects-sorter@1100.0.15
  - @pnpm/workspace.workspace-manifest-writer@1100.1.1
