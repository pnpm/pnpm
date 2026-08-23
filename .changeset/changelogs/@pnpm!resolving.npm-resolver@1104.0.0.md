## 1104.0.0

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

### Minor Changes

- A registry can now declare that its abbreviated metadata carries the `time` field, so `resolutionMode: time-based` reads the full metadata document only from the registries that need it:

  ```yaml
  resolutionMode: time-based
  registries:
    https://npm.internal.example/:
      supportsTimeField: true
  ```

  `registry.npmjs.org` omits `time` from abbreviated metadata, so a time-based resolution has to fall back to the much larger full document. That fallback used to be all-or-nothing: `registrySupportsTimeField` answered for every registry at once, so a project resolving from both the public registry and a Verdaccio instance either paid for full metadata everywhere or claimed a `time` field npmjs does not serve. The answer is now per registry, and `registrySupportsTimeField` remains the answer for every registry that does not declare one.

  The declaration is also sent to a pnpr server, which applies it to the resolution it runs on the client's behalf.

### Patch Changes

- Re-fetch full registry metadata when `minimumReleaseAge` is enabled and an abbreviated packument's `time` map omits timestamps for some versions. This prevents mature versions from being filtered out and resolution from falling back to the lowest matching version [pnpm/pnpm#13741](https://github.com/pnpm/pnpm/issues/13741).

- Reduced registry metadata requests during dependency resolution by reusing cached metadata when lockfile preferences prove that no uncached version can win [pnpm/pnpm#13976](https://github.com/pnpm/pnpm/issues/13976).

- Installs are faster in workspaces that declare inter-workspace dependencies with plain ranges (`"*"`, `"^1.2.3"`) rather than the `workspace:` protocol. With `preferWorkspacePackages` enabled, linking such a dependency no longer makes a registry request that cannot change the outcome — and workspace packages that were never published no longer cost a 404 on every install.

- Added `fetchWarnTimeoutMs` and `fetchMinSpeedKiBps` to the Rust pnpm CLI and its N-API bindings. Slow registry metadata requests and tarball downloads now emit pnpm-compatible warnings without exposing URL credentials, query parameters, fragments, or control characters [pnpm/pnpm#12042](https://github.com/pnpm/pnpm/issues/12042).

- `trustPolicy: no-downgrade` no longer aborts the install with `ERR_PNPM_MISSING_TIME` on registries that serve no per-version `time` field when `minimumReleaseAgeIgnoreMissingTime` is set. The trust check reads the same publish dates the `minimumReleaseAge` check does, so it now honors the same opt-in and skips the affected package with a warning [#12446](https://github.com/pnpm/pnpm/issues/12446).

  `minimumReleaseAgeIgnoreMissingTime` no longer lets a lockfile entry the registry does not list pass the `minimumReleaseAge` check during lockfile verification. The opt-in covers a registry that cannot date its releases; a packument that does date every version it lists is saying it never published this one, which stays a hard failure.

  The missing-`time` warning now names the check it is reporting on, so a package whose `minimumReleaseAge` and `trustPolicy` checks are both skipped warns about both instead of only the first.

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.0
  - @pnpm/config.pick-registry-for-package@1101.0.0
  - @pnpm/config.version-policy@1100.2.1
  - @pnpm/constants@1102.0.0
  - @pnpm/core-loggers@1100.3.3
  - @pnpm/deps.path@1101.0.0
  - @pnpm/error@1100.1.3
  - @pnpm/pkg-manifest.utils@1100.4.1
  - @pnpm/resolving.jsr-specifier-parser@1100.0.6
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.17
  - @pnpm/resolving.registry.types@1100.1.10
  - @pnpm/resolving.resolver-base@1101.1.1
  - @pnpm/store.cafs@1100.2.0
  - @pnpm/store.index@1100.2.5
  - @pnpm/types@1102.0.0
