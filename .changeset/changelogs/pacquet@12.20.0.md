## 12.20.0

### Minor Changes

- With `lockfile.format: v13`, `pnpm install` records the dependency specifiers of local directory packages. Frozen installs reject changed specifiers even when the locked versions still satisfy them.

### Patch Changes

- The `node-gyp` bundled with pnpm no longer ships undici 6, which security scanners flag for CVE-2026-84947. On Node.js 26, `node-gyp` downloads through a proxy or with a custom CA file no longer crash with "invalid onError method" [pnpm/pnpm#17941](https://github.com/pnpm/pnpm/issues/17941).

- Fixed `pnpm` and `pnpr` occasionally crashing on the first HTTPS request, mostly on Windows virtual machines under load. They now seed TLS randomness from the operating system's random number generator [pnpm/tasks#239](https://github.com/pnpm/tasks/issues/239).

- A root `overrides` entry that points a package at a `workspace:` path, such as `workspace:./packages/foo`, now also applies when a registry package depends on that package [#270](https://github.com/pnpm/tasks/issues/270).

- pnpm now recognizes Capawesome Cloud and RWX as CI environments.

- `pnpm sbom` now reports malformed workspace manifests instead of omitting their packages [pnpm/tasks#59](https://github.com/pnpm/tasks/issues/59).

- Fixed `pnpm deprecate`, `pnpm undeprecate`, and `pnpm unpublish` failing with revision conflicts when a registry cache serves stale package metadata.

- pnpr now advertises capabilities through `pnpr.protocols` at the server root and npm registry bases. pnpm reads this capability map for remote resolution and shared build caches. The HTTP protocol path remains `/v0/`.

- Conflicting non-semver peer dependencies now remain missing when `autoInstallPeersFromHighestMatch` is enabled. Identical non-semver peer specifiers are preserved. Preferred registry versions no longer replace local paths or protocol-prefixed specifiers when installing missing peers [pnpm/tasks#272](https://github.com/pnpm/tasks/issues/272).

- Git dependencies now accept npm's `::` separator between fragment parameters, such as `github:owner/repo#v1.0.0::path:packages/foo`. Before, pnpm read the whole fragment as a branch name and failed to resolve it [pnpm/pnpm#17827](https://github.com/orgs/pnpm/discussions/17827).

- Fixed `pnpm store status` reporting approved builds of git dependencies as modified when the store contains legacy build metadata.

- With `nodeLinker: hoisted` and `autoInstallPeers: false`, a package whose peer dependency its parent supplies at a version other than the top-level one now stays nested under that parent, even when a copy of the same package version is already at the top level. Before, the nested copy was removed as a duplicate, so the package resolved its peer to the top-level version.

- With `nodeLinker: hoisted`, when two peer variants of one package version compete for a top-level `node_modules` slot, the variant found under a direct dependency wins over one found deeper in the graph. The winning variant's own `node_modules` keeps the dependencies it resolved at a version other than the top-level one. Before, the deeper variant could take the slot and that nested dependency was missing, so the package loaded a version of it that its dependent did not expect.

- `pnpm dedupe` now keeps a locked runtime version, from `devEngines.runtime` or from a dependency's `engines.runtime`, when the declared range still admits it. `pnpm dedupe --check` no longer fails each time a newer Node.js release in the range comes out [#17984](https://github.com/pnpm/pnpm/issues/17984).

- Installs now fail when a pnpr server does not confirm the requested resolution and verification policies. Upgrade the pnpr server before upgrading pnpm.

- Fixed writing `lockfile.format: v13` for dependency cycles with missing, optional, or transitive peers. Frozen installs also accept unchanged injected packages whose peers use catalogs. With `--no-optional`, frozen installs reject required catalog children recorded only as optional dependencies [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

- `pnpm deploy` preserves an injected workspace package's resolved dependencies when they differ from the workspace project's own dependencies. This also applies to packages with `publishConfig.directory` [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

- Fixed `pnpm install` keeping stale peer bindings after dependency changes. Required dependencies no longer remain marked optional when another workspace project removes a shared package [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

- `pnpm outdated --notes` and `pnpm update -i` now read the `namespace` field on publication links in `<name>/-/notes` responses from `serverType: pnpr` registries [zkochan/pnpm-tasks#66](https://github.com/zkochan/pnpm-tasks/issues/66).

- When a command name matches no script and pnpm falls back to running it as a command, the install that runs before scripts now runs once. Its errors and warnings print once [#17977](https://github.com/pnpm/pnpm/issues/17977).

- Failed downloads by project-aware global runtime commands now explain which configuration applies. Node.js failures also list the global mirror settings and environment variables to check.

- Repeated installs now keep the same dependency links when packages form cycles with several peer dependency variants [pnpm/tasks#221](https://github.com/pnpm/tasks/issues/221).

- Version-selector `overrides` entries such as `"ms@^2.0.0": "2.1.1"` now apply to dependencies declared with the `catalog:` protocol when the catalog entry's range matches the selector [#17922](https://github.com/pnpm/pnpm/issues/17922).

- Scripts that pnpm runs in a process group of their own are now ended when pnpm is killed right after starting them. A parallel run that was killed before pnpm registered its scripts left all of them running.
