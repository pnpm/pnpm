## 12.20.0

pnpm 12.20.0 records the specifiers of local directory dependencies in the experimental v13 lockfile and fixes several `nodeLinker: hoisted`, peer dependency, and `overrides` bugs. The bundled `node-gyp` no longer ships the undici version that security scanners flag. Remote resolution and shared build caches now need pnpr 0.1.0-alpha.20 or newer, so upgrade the pnpr server before upgrading pnpm.

### Minor Changes

- With `lockfile.format: v13`, `pnpm install` records the dependency specifiers of local directory packages. Frozen installs reject changed specifiers even when the locked versions still satisfy them.

### Patch Changes

#### Upgrading and security

- pnpm now requires pnpr 0.1.0-alpha.20 or newer for remote resolution, shared build caches, and release notes. pnpm reads the capabilities a pnpr server advertises under `pnpr.protocols`, and installs fail when the server does not confirm the requested resolution and verification policies. Upgrade the pnpr server before upgrading pnpm.

- The `node-gyp` bundled with pnpm no longer ships undici 6, which security scanners flag for CVE-2026-84947. On Node.js 26, `node-gyp` downloads through a proxy or with a custom CA file no longer crash with "invalid onError method" [#17941](https://github.com/pnpm/pnpm/issues/17941).

#### Resolving dependencies

- Version-selector `overrides` entries such as `"ms@^2.0.0": "2.1.1"` now apply to dependencies declared with the `catalog:` protocol when the catalog entry's range matches the selector [#17922](https://github.com/pnpm/pnpm/issues/17922).

- A root `overrides` entry that points a package at a `workspace:` path, such as `workspace:./packages/foo`, now also applies when a registry package depends on that package [pnpm/tasks#270](https://github.com/pnpm/tasks/issues/270).

- Git dependencies now accept npm's `::` separator between fragment parameters, such as `github:owner/repo#v1.0.0::path:packages/foo`. Before, pnpm read the whole fragment as a branch name and failed to resolve it [#17827](https://github.com/orgs/pnpm/discussions/17827).

- `pnpm dedupe` now keeps a locked runtime version, from `devEngines.runtime` or from a dependency's `engines.runtime`, when the declared range still admits it. `pnpm dedupe --check` no longer fails each time a newer Node.js release in the range comes out [#17984](https://github.com/pnpm/pnpm/issues/17984).

- With `autoInstallPeersFromHighestMatch`, conflicting non-semver peer dependencies now remain missing, and identical non-semver peer specifiers are preserved. Preferred registry versions no longer replace local paths or protocol-prefixed specifiers when pnpm installs missing peers [pnpm/tasks#272](https://github.com/pnpm/tasks/issues/272).

#### Hoisted node linker

- With `nodeLinker: hoisted`, a package now keeps its nested copy of a dependency that it resolved at a version other than the top-level one, including a peer dependency its parent supplies. Before, pnpm could remove that copy as a duplicate or give the top-level slot to a peer variant found deeper in the graph. The package then loaded the top-level version.

#### Lockfile and peer dependency graphs

- Fixed `pnpm install` keeping stale peer bindings after dependency changes. Required dependencies no longer remain marked optional when another workspace project removes a shared package [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

- Repeated installs now keep the same dependency links when packages form cycles with several peer dependency variants [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

- Fixed writing `lockfile.format: v13` for dependency cycles with missing, optional, or transitive peers. Frozen installs also accept unchanged injected packages whose peers use catalogs. With `--no-optional`, frozen installs reject required catalog children recorded only as optional dependencies [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

- `pnpm deploy` preserves an injected workspace package's resolved dependencies when they differ from the workspace project's own dependencies. This also applies to packages with `publishConfig.directory` [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

#### Running scripts and runtimes

- Scripts that pnpm runs in a process group of their own are now ended when pnpm is killed right after starting them. A parallel run that was killed before pnpm registered its scripts left all of them running.

- When a command name matches no script and pnpm falls back to running it as a command, the install that runs before scripts now runs once. Its errors and warnings print once [#17977](https://github.com/pnpm/pnpm/issues/17977).

- Failed downloads by project-aware global runtime commands now explain which configuration applies. Node.js failures also list the global mirror settings and environment variables to check.

#### Other commands and settings

- Fixed `pnpm` occasionally crashing on the first HTTPS request, mostly on Windows virtual machines under load. pnpm now seeds TLS randomness from the operating system's random number generator [pnpm/tasks#239](https://github.com/pnpm/tasks/issues/239).

- Fixed `pnpm deprecate`, `pnpm undeprecate`, and `pnpm unpublish` failing with revision conflicts when a registry cache serves stale package metadata.

- `pnpm sbom` now reports malformed workspace manifests. Before, it omitted their packages [pnpm/tasks#59](https://github.com/pnpm/tasks/issues/59).

- Fixed `pnpm store status` reporting approved builds of git dependencies as modified when the store contains legacy build metadata.

- pnpm now recognizes Capawesome Cloud and RWX as CI environments.
