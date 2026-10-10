## 12.12.0

This release shares task results and dependency builds through a remote cache, adds the `addMissingPeerTypes` setting, and lets `pnpm add --config` and `pnpm update` manage configuration dependencies. It also fixes `dangerouslyAllowAllBuilds` running the build scripts of packages that `allowBuilds` denies.

### Minor Changes

- `pnpm pipeline` can share task results between machines. A result is a signed artifact stored on a pnpr server or on a server that speaks the Turborepo Remote Cache API, such as Vercel Remote Cache.

  The remote side-effects cache can also store dependency builds on a Turborepo Remote Cache server. Both caches read the new `remoteCache` setting, which names the server and holds the signing keys. `pnpm pipeline --report` records the run under the organization `remoteCache.org` names.

- Added the `addMissingPeerTypes` setting, which links the `@types` package of a peer dependency next to every package that peer-depends on it. For example, a package that peer-depends on `react` gets the project's `@types/react` linked next to it. This lets TypeScript find the types of a peer dependency when the global virtual store is on [#15689](https://github.com/pnpm/pnpm/discussions/15689).

- `pnpm add --config` and `pnpm update` now handle configuration dependencies the way they handle other dependencies [#16814](https://github.com/pnpm/pnpm/issues/16814).

  `pnpm add --config` saves a version range in `pnpm-workspace.yaml` by the same rules as `pnpm add`, so `pnpm add --config my-config@latest` saves a range such as `^1.2.0`. It also resolves the package again when `pnpm-lock.yaml` already locks an older version.

  `pnpm update` updates configuration dependencies within their ranges, and `pnpm update --latest` moves them to their latest version. A selector such as `pnpm update my-config` updates only the configuration dependencies it names.

### Patch Changes

- A package set to `false` in `allowBuilds` no longer runs its build scripts when `dangerouslyAllowAllBuilds` is `true`. The two settings together now allow every build except the denied ones. Previously, `dangerouslyAllowAllBuilds` ignored the denials.

- pnpm now switches to the pnpm version a project pins even when it cannot read its configuration. Previously, a setting it rejected in the global config file, an `.npmrc` file, or `_auth`, or a `pnpm-workspace.yaml` it could not parse, stopped every command, `pnpm --version` included.

- `pnpm update` and `pnpm why` accept `--depth Infinity` again. They failed with `invalid value 'Infinity' for '--depth <DEPTH>'` [#16817](https://github.com/pnpm/pnpm/issues/16817).

- `pnpm deploy` without `injectWorkspacePackages` now binds a peer dependency of a linked workspace package to the version its parent provides, as an injected install does. Previously the deploy failed with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` whenever other packages in the deployed graph depended on different versions of that peer [#16807](https://github.com/pnpm/pnpm/issues/16807).

- `pnpm deploy --legacy` no longer fails with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` when a workspace dependency of the deployed project declares a `workspace:` peer dependency that the project does not depend on itself. pnpm now copies that peer from the workspace into the deploy directory.

  An auto-installed peer declared with `workspace:` now always resolves to the workspace project. Previously, `workspace:*` and `workspace:^1.0.0` peers could be installed from the registry [#16806](https://github.com/pnpm/pnpm/issues/16806).

- Repeat installs no longer resolve again when an injected workspace package lists a dependency also as a peer dependency.

- Sped up frozen installs of unchanged injected workspace packages that use hardlinks.

- Sped up workspace discovery with subtree exclusions such as `!generated/**`.

- A key under a registry URL in the `_auth` setting that does not start with `@` is now skipped with a warning, so a field a later pnpm version adds there no longer breaks this one. A key that holds an `authToken` object but lacks its `@`, such as `org` for `@org`, is still an error.
