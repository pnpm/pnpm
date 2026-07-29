## 1100.4.0

### Minor Changes

- `overrides` now also govern peers that pnpm auto-installs. Previously an override only rewrote dependencies declared in a manifest, so a peer nobody declares — installed because `autoInstallPeers` is on — resolved against its declared peer range and could bring in a second copy of the very package the override pinned. For example, with `overrides: { react: npm:react@19.2.0 }` and a lone `lucide-react` dependency, pnpm installed `react@18.3.1`; it now installs the pinned `react@19.2.0` [#13320](https://github.com/pnpm/pnpm/issues/13320).

### Patch Changes

- Installing a local `file:` directory dependency with the global virtual store enabled no longer fails with `TypeError: Cannot read properties of undefined (reading 'split')` [#13335](https://github.com/pnpm/pnpm/issues/13335).

  Local directory dependencies — `file:` directories and injected workspace packages — now get a global-virtual-store slot of their own per project. They used to share one slot across every project that depended on a directory of the same name, so a project could end up linked to another project's copy of the dependency.

- An auto-installed *optional* peer is no longer hoisted at a version the workspace root's own dependency on that package excludes. `resolvePeersFromWorkspaceRoot` already made the workspace root's specifier decide which version a missing *required* peer is installed at; the optional-peer picker ignored it and always took the highest version present anywhere in the graph. In a workspace whose root pins `postcss: 8.5.10`, an importer that depends on `webpack` and declares no `postcss` of its own got `postcss@8.5.22` hoisted for `terser-webpack-plugin`'s optional `postcss` peer, leaving two `postcss@8.5.x` instances in the graph [#13320](https://github.com/pnpm/pnpm/issues/13320).

- Under `resolvePeersFromWorkspaceRoot`, a workspace root dependency declared with `link:` or `file:` (or the path form of `workspace:`, such as `workspace:../pkg`) now satisfies another project's missing peer dependency at the linked package's own version, instead of being hoisted as a path. Those specifiers are relative to the project that declares them, so the same specifier reached a different directory — or none — from the project the peer was hoisted into, leaving a broken link. The root now has the same authority over the peer as it has when it declares the package with a version range [#13373](https://github.com/pnpm/pnpm/issues/13373).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.1.10
  - @pnpm/core-loggers@1100.3.0
  - @pnpm/deps.graph-hasher@1100.2.13
  - @pnpm/deps.path@1100.0.12
  - @pnpm/fetching.pick-fetcher@1100.1.4
  - @pnpm/fs.symlink-dependency@1100.0.15
  - @pnpm/hooks.types@1100.2.4
  - @pnpm/lockfile.preferred-versions@1100.0.26
  - @pnpm/lockfile.pruner@1100.0.17
  - @pnpm/lockfile.types@1100.0.17
  - @pnpm/lockfile.utils@1100.1.6
  - @pnpm/patching.config@1100.0.13
  - @pnpm/pkg-manifest.reader@1100.0.13
  - @pnpm/pkg-manifest.utils@1100.2.13
  - @pnpm/resolving.npm-resolver@1102.1.9
  - @pnpm/resolving.resolver-base@1100.5.5
  - @pnpm/store.controller-types@1100.1.11
  - @pnpm/types@1101.7.0
