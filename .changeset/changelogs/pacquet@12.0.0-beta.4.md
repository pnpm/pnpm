## 12.0.0-beta.4

### Minor Changes

- **Security fix.** Affects projects using `namedRegistries` on pnpm 11.1.0–11.19.x. It is **semi-breaking** for those projects — see "If you use named registries" below.

  The lockfile recorded no marker for which registry a package came from. Packages were keyed by `name@version` alone, and entry lookup went through `refToRelative(ref, name)`, so a dependency you declared against one registry could be satisfied by an entry that was actually resolved from another. When two registries served the same name and version, both collapsed onto a single `packages:` entry and whichever resolved first decided the tarball every consumer got.

  That is a package-substitution risk: a package you expect from your private registry could be installed from a different registry that publishes the same name and version, and the lockfile recorded nothing that would let you tell.

  Packages resolved from a named registry are now recorded under registry-qualified keys (`<name>@<registryName>:<version>`, e.g. `foo@work:1.0.0`), so each registry gets its own entry and the lockfile pins which one a dependency came from.

  The lockfile format version is unchanged. Registry-qualified keys appear only for packages resolved from a named registry, so a project that does not use `namedRegistries` sees no difference, and older pnpm versions keep reading the file.

  ### If you use named registries

  Your next non-frozen install re-keys those entries, which shows up as a lockfile diff. Commit it — that diff is the fix being applied. Review it: an entry that moves to a registry you did not expect is worth investigating.

  Everyone working on the project should be on this version or newer before you do. An older pnpm reads the re-keyed lockfile fine — frozen installs are unaffected — but it does not produce registry-qualified keys itself, so any install that updates the lockfile writes those entries back to the old shape, and the next install on a current pnpm re-qualifies them. The result is a lockfile that flips back and forth, and while it is in the old shape the project is exposed again. Because the lockfile format version is deliberately unchanged, pnpm cannot detect this and warn you about it.

  There is no setting to keep the old behavior: the old shape is the vulnerability.

  Tarball URLs that follow the standard registry layout are no longer written to the lockfile for named-registry packages; they are recomputed from the `namedRegistries` setting on demand.

  To use named registries, map your aliases in `pnpm-workspace.yaml`:

  ```yaml
  namedRegistries:
    work: https://npm.enterprise.example.com/
  ```

  ### New built-in `npmjs:` alias

  `npmjs:` now resolves to `https://registry.npmjs.org/` with no configuration, alongside the existing `gh:` alias for GitHub Packages. It pins a dependency to the public registry even when `registry` points elsewhere, such as an internal proxy:

  ```json
  { "dependencies": { "left-pad": "npmjs:^1.3.0" } }
  ```

  `npm:` cannot do this — it is the alias protocol (`npm:<name>@<range>`) and resolves through whatever `registry` points at.

  **If you mirror or proxy npmjs, point the alias at your mirror:**

  ```yaml
  namedRegistries:
    npmjs: https://npm.internal.example.com/
  ```

  Built-in registry URLs are also the prefixes a lockfile's recorded tarball URL is matched against when pnpm verifies a package. Without the override, an entry whose tarball URL is on `registry.npmjs.org` is verified against the public registry rather than your mirror. This only affects lockfiles that record such URLs — a canonical URL for your configured registry is omitted from the lockfile and unaffected — and only when a tarball-URL, `minimumReleaseAge`, or `trustPolicy` check runs. Overriding the alias is the same escape hatch GHES users already have for `gh`.

  Every alias the lockfile references must stay in `namedRegistries`: reading an entry whose alias is gone fails with `ERR_PNPM_MISSING_NAMED_REGISTRY` rather than silently falling back to the default registry, since that would fetch a different package. Renaming an alias re-resolves the packages that used it.

  Named registry aliases that shadow a reserved dependency specifier prefix (`file`, `link`, `workspace`, `runtime`, `npm`, `jsr`, ...) are now rejected with `ERR_PNPM_RESERVED_NAMED_REGISTRY_NAME` instead of being silently shadowed by the corresponding resolver.

  `pnpm licenses` and `pnpm sbom` now keep the two artifacts apart as well: license records carry the registry alias, and SBOM components carry the purl `repository_url` qualifier.

### Patch Changes

- Installing a workspace whose projects auto-install peer dependencies is substantially faster. Each round of the peer-hoist loop no longer scans the whole workspace once per project, so the cost of resolution grows with the workspace instead of with its square.

- Installing a dependency chain whose packages carry peer dependencies no longer expands exponentially with the depth of the chain. A single project with a single such dependency could exhaust memory before finishing; it now resolves in tens of megabytes.

- Fixed non-deterministic resolution on multi-project workspaces: two consecutive installs of the same inputs could bind peer-suffixed packages to different (still valid) providers, rewriting `pnpm-lock.yaml` on every install [#13567](https://github.com/pnpm/pnpm/issues/13567).

- Installing a workspace now produces the same `pnpm-lock.yaml` every time. Two installs of the same workspace could previously bind a peer dependency to a different — still valid — version, which changed the lockfile without anything in the project changing.

- An empty `http-proxy`, `https-proxy`, `proxy`, or `no-proxy` value — from the `.npmrc`, `pnpm-workspace.yaml`, the CLI, or the `HTTP_PROXY` / `HTTPS_PROXY` / `PROXY` / `NO_PROXY` environment variables — no longer fails the install with `ERR_PNPM_INVALID_PROXY`. Empty settings read as unset, so a shell exporting `HTTP_PROXY=` disables the proxy, and an empty `proxy=` in the `.npmrc` no longer suppresses `HTTPS_PROXY` [#13533](https://github.com/pnpm/pnpm/issues/13533).

  `proxy=false` in the `.npmrc` or `proxy: false` in `pnpm-workspace.yaml` now turns proxying off instead of being read as a proxy host named `false`. `false` and `null` on `https-proxy` / `http-proxy` / `no-proxy` read as unset, and on the command line they are ordinary host names, since a flag carries its value verbatim.

- The env lockfile no longer pins `@pnpm/exe` alongside `pnpm` when the wanted pnpm version is 12 or newer. From v12 the unscoped `pnpm` package is itself the native executable, so `@pnpm/exe` is not published for it and resolving it would fail. The engine identity check now verifies the native binary through whichever package ships it.

- Resolution on large peer-heavy workspaces got faster: a Bit workspace with 114 projects and ~21,000 lockfile entries resolves in ~13.4s instead of ~16.0s. The resolved dependency graph is unchanged.

- Fixed nondeterministic peer bindings in large multi-project workspaces.

- Resolving a workspace whose dependency chains are deep is faster: deciding which missing peer dependencies another project's resolution already covers now answers once per shared chain segment instead of once per report.

- Peer resolution on large workspaces got faster: each hoist round now refreshes its view of the dependency graph from what the round changed instead of re-reading every resolved package. The resolved dependency graph is unchanged.

- `pnpm install` no longer crashes on a machine whose system certificate store is empty or absent — for example a minimal container or build sandbox that ships no CA certificates [#13588](https://github.com/pnpm/pnpm/issues/13588). Such a system now falls back to the Mozilla root certificates bundled into the binary, the same set Node.js ships, so both offline and online installs work again. Certificates from the system store, `NODE_EXTRA_CA_CERTS`, and the `.npmrc` `ca` / `cafile` settings keep taking precedence whenever any of them is available.

- Fixed the order in which pnpm matches a lockfile's recorded tarball URL against known registry URLs. Two registry URLs of equal length were previously ordered arbitrarily, so which one a tarball URL matched could differ between runs.

- `pnpm login` / `pnpm adduser` now read the `scope` setting from `pnpm-workspace.yaml`, the global `config.yaml`, and the `PNPM_CONFIG_SCOPE` environment variable, not only from the `--scope` command-line flag. When `scope` is configured, the granted token is keyed to that scope and the scope-to-registry mapping is recorded. `--scope` still takes precedence when both are set. Note that `scope` in an `.npmrc` is not read — pnpm keeps only auth and registry keys from that file.

- Resolution spends less time in its final peer pass: the package-name cycle graph it consults is now derived once per package instead of once per occurrence of that package.

- npm's `--prefix` is accepted as a spelling of `--dir`, and `--store` as a spelling of `--store-dir`, so `pnpm --prefix ../ run test` no longer fails with "unexpected argument '--prefix' found" [#13583](https://github.com/pnpm/pnpm/issues/13583).

- pnpm now ships `node-gyp` again, so packages whose install scripts shell out to it build out of the box. Previously they failed with `spawn node-gyp ENOENT` unless a `node-gyp` was already on `PATH` — affecting `node-gyp-build` with no matching prebuild, `node-pre-gyp`, a plain `"install": "node-gyp rebuild"`, and any package shipping a `binding.gyp` without an install script. As in pnpm 11, the whole `node-gyp` dependency tree is resolved from pnpm's own lockfile when pnpm is released, so it is frozen per release rather than resolved on your machine, and `npm_config_node_gyp`, a workspace `node-gyp`, and a package's own `node-gyp` dependency all still take precedence.
