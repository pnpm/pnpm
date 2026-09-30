## 12.8.2

### Patch Changes

- `childConcurrency` now defaults to 5, the documented value. It used to be capped at 4 and to follow the host's CPU count.

- Run concurrent resolution for identical wanted keys once, coalescing simultaneous first misses across level siblings and avoiding repeated hook executions.

- `pnpm config set --location=project` refuses a machine-level setting such as `stateDir` or `scope` with `ERR_PNPM_CONFIG_SET_NOT_A_PROJECT_SETTING`, which names where the setting belongs. `pnpm config delete` still clears such a key from a project's `pnpm-workspace.yaml`.

- `pnpm dedupe --check` now passes right after `pnpm dedupe` when deduplication merges variants of a package that differ only in their peers. A lockfile key whose peer suffix named a merged variant now names the variant that replaced it [#16356](https://github.com/pnpm/pnpm/issues/16356).

- `pnpm deploy` no longer copies the workspace root's `packageManager` and `devEngines.packageManager` fields into the deployed `package.json` [#16403](https://github.com/pnpm/pnpm/issues/16403).

- `pnpm deploy` no longer fails with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` in a workspace with `injectWorkspacePackages: true` when a workspace package lists its peer dependency as a dev dependency too [#16375](https://github.com/pnpm/pnpm/issues/16375).

- `pnpm store prune` now removes the packages that only expired `pnpm dlx` cache entries used. They were left in the store until the next `pnpm store prune` [#16383](https://github.com/pnpm/pnpm/discussions/16383).

- Fixed pnpm crashing on startup on Linux ppc64le [#16380](https://github.com/pnpm/pnpm/issues/16380).

- Enable `zlib-rs` feature for decompression backend to improve package extraction performance.

- Sped up `pnpm install` with `nodeLinker: hoisted` on macOS when the lockfile is re-resolved, such as with `autoDedupe` enabled [#16397](https://github.com/pnpm/pnpm/issues/16397).

- `pnpm install --frozen-lockfile` now fails when `Cargo.lock` does not satisfy a dependency requirement in `Cargo.toml`. The error names the crate and the version the lockfile holds [pnpm/pnpm#16355](https://github.com/pnpm/pnpm/issues/16355).

- `pnpm run` and `pnpm exec` no longer install dependencies before every script on CI when `autoDedupe` is enabled. `pnpm install --frozen-lockfile` now keeps the deduplication record left by an earlier install [#16374](https://github.com/pnpm/pnpm/issues/16374).

- Git-hosted dependencies now respect `pmOnFail`. If it is set to anything other than `download`, a git-hosted dependency that pins a pnpm version is prepared by the running pnpm, and pnpm does not download the pinned version [#16376](https://github.com/pnpm/pnpm/issues/16376).

- `pnpm config get globalShims`, `pnpm shim list`, and global installs no longer read `globalShims` from a project's `pnpm-workspace.yaml`. Only the global config file, the pnpm home's own `pnpm-workspace.yaml`, and `PNPM_CONFIG_GLOBAL_SHIMS` set it, so a repository cannot choose which globally installed packages get project-aware shims.

- Sped up resolution when many dependencies request different ranges of the same package. pnpm now checks each version's deprecation status once.

- With `injectWorkspacePackages: true`, a fresh `pnpm install` now records a workspace dependency as `link:` when its injected copy differs from the project only by an optional peer that peer-dependent dedupe merges. It was recorded as a peer-suffixed `file:` copy [#16354](https://github.com/pnpm/pnpm/issues/16354).

- `pnpm install` without `--frozen-lockfile` is faster on some machines in projects with a `pnpm-workspace.yaml`. Those installs linked with one worker thread per core, half of what a frozen install uses.

- When `minimumReleaseAge` hides the version that `latest` points to, pnpm now falls back to a prerelease of the same major before a stable version of an older major. A stable version of the same major is still preferred. Previously, while a new `1.0.0` was too new, `latest` fell back to an old `0.0.1` even though `1.0.0-beta.4` had been `latest` until then [#16388](https://github.com/pnpm/pnpm/issues/16388).

- On macOS and Linux, lifecycle scripts and `pnpm run` now always get `PATH` from the `PATH` variable. When the environment also held a `Path` variable, a script sometimes got `Path`'s value, and failed with `node: not found` [#16308](https://github.com/pnpm/pnpm/issues/16308).

- `pnpm run` now exits after a `SIGTERM` in a container where pnpm is PID 1 and the script runs pnpm again, as `"start": "pnpm serve"` does. It used to keep waiting after the script had shut down, until the container runtime killed it.

- Fixed installs failing with `UnknownIssuer` on Linux systems without CA certificates, such as `node:24-slim`, when `NODE_EXTRA_CA_CERTS` is set. The extra certificates now extend the bundled CA roots [#16365](https://github.com/pnpm/pnpm/issues/16365).

- `pnpm publish` now includes bare `README` files and README files with Markdown extensions such as `readme.markdown` in registry metadata [#12704](https://github.com/pnpm/pnpm/issues/12704).

- `pnpm install` returns "Already up to date" again in a workspace with injected workspace dependencies and a shared lockfile. Since v12.7.0 every repeat install in such a workspace ran the full install and copied the injected projects again.

- Sped up dependency resolution in large workspaces. The resolver now shares one copy of each package id between every node, ancestor chain and cache entry that refers to it, and renders a package's name and version once for all of its peer dependency checks.

- POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build, where the system default path holds none of the utilities they call. Installing again replaces the shims already in `node_modules` [#16377](https://github.com/pnpm/pnpm/issues/16377).

- pnpm now creates its store operation locks and other per-user lock files in `$XDG_RUNTIME_DIR` when it points to a directory only the user can write to. Otherwise, pnpm still uses `/tmp` on Linux and macOS. Sandboxes that block writes to `/tmp` can point `XDG_RUNTIME_DIR` at a writable directory [#16390](https://github.com/pnpm/pnpm/issues/16390).

- In a project that pins another pnpm version, pnpm now passes a command with an option it does not know to the pinned version. Before, pnpm rejected the option before switching, so `pnpm install --auto-dedupe` failed with "Unknown option" even though the pinned pnpm supports it [#16353](https://github.com/pnpm/pnpm/issues/16353).

- On Windows, warm `pnpm install --frozen-lockfile` runs are 4-5% faster on 4- and 8-core machines. pnpm now links with one worker thread per core on Windows, between 4 and 16. This changes frozen installs and installs in projects without a `pnpm-workspace.yaml` on machines with 3 to 15 cores.
