## 11.20.0

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

- An empty `http-proxy`, `https-proxy`, `proxy`, or `no-proxy` value — from the `.npmrc`, `pnpm-workspace.yaml`, the CLI, or the `HTTP_PROXY` / `HTTPS_PROXY` / `PROXY` / `NO_PROXY` environment variables — no longer fails the install with `ERR_PNPM_INVALID_PROXY`. Empty settings read as unset, so a shell exporting `HTTP_PROXY=` disables the proxy, and an empty `proxy=` in the `.npmrc` no longer suppresses `HTTPS_PROXY` [#13533](https://github.com/pnpm/pnpm/issues/13533).

  `proxy=false` in the `.npmrc` or `proxy: false` in `pnpm-workspace.yaml` now turns proxying off instead of being read as a proxy host named `false`. `false` and `null` on `https-proxy` / `http-proxy` / `no-proxy` read as unset, and on the command line they are ordinary host names, since a flag carries its value verbatim.

- The env lockfile no longer pins `@pnpm/exe` alongside `pnpm` when the wanted pnpm version is 12 or newer. From v12 the unscoped `pnpm` package is itself the native executable, so `@pnpm/exe` is not published for it and resolving it would fail. The engine identity check now verifies the native binary through whichever package ships it.

- `lexCompare` and `nerfDart` are now published as `@pnpm/text.ordinal-comparator` and `@pnpm/config.registry-auth-key`. Use these instead of `@pnpm/util.lex-comparator` and `@pnpm/config.nerf-dart`.

- Fixed the order in which pnpm matches a lockfile's recorded tarball URL against known registry URLs. Two registry URLs of equal length were previously ordered arbitrarily, so which one a tarball URL matched could differ between runs.

- Dependency resolution is faster: package metadata is now filtered once per packument instead of once per dependency edge when `minimumReleaseAge` is active, and parsed semver versions and ranges are reused instead of re-parsed on every comparison.

- Security: `pnpm rebuild` now refuses a lockfile whose `packages` key carries a path traversal in the package name (e.g. `../../../escaped@1.0.0`), instead of running that package's lifecycle scripts and linking its bins in a directory outside the virtual store. Such a name is rejected with `ERR_PNPM_INVALID_DEPENDENCY_NAME`.
