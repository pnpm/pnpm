export type DependenciesField = 'optionalDependencies' | 'dependencies' | 'devDependencies'

export type DependenciesOrPeersField = DependenciesField | 'peerDependencies'

// NOTE: The order in this array is important.
export const DEPENDENCIES_FIELDS: DependenciesField[] = [
  'optionalDependencies',
  'dependencies',
  'devDependencies',
]

export const DEPENDENCIES_OR_PEER_FIELDS: DependenciesOrPeersField[] = [
  ...DEPENDENCIES_FIELDS,
  'peerDependencies',
]

export interface RegistriesByScope {
  default: string
  [scope: string]: string
}

/**
 * The required keys are the enforcement: a raw `registriesByPrefix` from user
 * config does not satisfy this type, so it cannot reach a consumer without
 * passing through `normalizeRegistriesByPrefix`. Widening it is how adding a
 * built-in gets acknowledged.
 */
export interface RegistriesByPrefix {
  gh: string
  npmjs: string
  [name: string]: string
}

/**
 * The software serving a registry, declared through the `registries` setting.
 * Three states, because "behaves like the npm registry" is a claim only the
 * operator can make:
 *
 * - undeclared — strict. Only the exact canonical URL is reconstructible.
 *   This is how every registry but registry.npmjs.org is read by default.
 * - `npm` — behaves like registry.npmjs.org, which serves a scoped package
 *   from the percent-encoded path as well as the unencoded one. A faithful
 *   mirror or caching proxy of the public registry is this.
 * - `artifactory` — repeats the scope in a scoped package's tarball filename.
 *
 * Only layouts pnpm can rebuild a URL for belong here. A registry that serves
 * tarballs from a content-derived path (GitHub Packages
 * `/download/<scope>/<name>/<version>/<sha256>`) has no value: the digest is a
 * fact about the bytes rather than about the package's identity, so its URLs
 * are kept in the lockfile instead.
 */
export type RegistryServerType = 'npm' | 'artifactory'

/**
 * Non-secret, per-registry settings from the `registries` setting. Held apart
 * from {@link RegistryConfig} so the install and lockfile layers can be handed
 * a registry's layout without also being handed its credentials.
 */
export interface RegistryOptions {
  serverType?: RegistryServerType
  /**
   * Whether this registry's abbreviated metadata carries the `time` field.
   *
   * `registry.npmjs.org` does not, which is why the default is `false` and why
   * a time-based resolution falls back to the far larger full metadata. A
   * registry that does carry it — Verdaccio and several proxies — is worth
   * declaring: the fallback is per registry, so one that needs full metadata
   * no longer costs it at the others.
   */
  supportsTimeField?: boolean
}

/**
 * One entry of the `registries` setting: everything a project declares about
 * a registry, keyed by its URL so each fact is stated once.
 *
 * `scopes` and `prefix` are the routes *to* the registry, and are inverted
 * into {@link RegistriesByScope} and {@link RegistriesByPrefix} when the config is read
 * — a scope resolves to one registry, but a registry serves many, so the
 * declaration reads the way it is written and each lookup keeps the shape it
 * is queried in.
 */
export interface RegistryDeclaration extends RegistryOptions {
  /**
   * The scopes routed here, `@`-prefixed. {@link DEFAULT_REGISTRY_SCOPE}
   * (a bare `@`) is the registry packages resolve from when no scope matches,
   * the same registry the `registry` setting names.
   */
  scopes?: string[]
  /**
   * The bare-specifier prefix this registry answers to, as in
   * `"foo": "work:^1.0.0"`. Singular because the prefix is the registry's
   * identity in a lockfile dep path (`foo@work:1.0.0`): a second spelling
   * would key the same package from the same registry twice.
   */
  prefix?: string
}

/**
 * Everything needed to decide which registry a package came from and what that
 * registry does: the scope-routed URLs, the `<name>:`-addressed aliases, and
 * the declared per-registry settings.
 *
 * Mixed into an options type (`RegistryContext & { … }`) rather than spelled
 * out field by field, so that a new per-registry setting reaches every
 * consumer by being added here. Forward it with `pickRegistryContext` for the
 * same reason: dropping a field is silent — the tarball URL is simply rebuilt
 * in the wrong layout — so neither end should be written out by hand.
 */
export interface RegistryContext {
  registriesByScope: RegistriesByScope
  /** As the user wrote it; built-in aliases are merged in at lookup. */
  registriesByPrefix?: Record<string, string>
  registryOptionsByUrl?: Record<string, RegistryOptions>
}

/** Parsed value of `_auth` of each registry in the rc file. */
export interface BasicAuth {
  username: string
  password: string
}

/** Parsed value of `tokenHelper` of each registry in the rc file. */
export type TokenHelper = [string, ...string[]]

export const DEFAULT_REGISTRY_SCOPE = '@'

/** Per-registry authentication credentials. */
export interface Creds {
  /** Parsed value of `_auth` of each registry in the rc file. */
  basicAuth?: BasicAuth
  /** The value of `_authToken` of each registry in the rc file. */
  authToken?: string
  /** Parsed value of `tokenHelper` of each registry in the rc file. */
  tokenHelper?: TokenHelper
}

/** Per-registry TLS configuration. */
export interface TlsConfig {
  /** Client certificate (PEM). */
  cert?: string
  /** Client private key (PEM). */
  key?: string
  /** Certificate authority (PEM). */
  ca?: string
}

/** Per-registry configuration (credentials + TLS). */
export interface RegistryConfig {
  [scope: `@${string}`]: Creds | undefined
  tls?: TlsConfig
}

export type HoistedDependencies = Record<DepPath | ProjectId, Record<string, 'public' | 'private'>>

export type PkgResolutionId = string & { __brand: 'PkgResolutionId' }

export type PkgId = string & { __brand: 'PkgId' }

export type PkgIdWithPatchHash = string & { __brand: 'PkgIdWithPatchHash' }

export type DepPath = string & { __brand: 'DepPath' }

export type ProjectId = string & { __brand: 'ProjectId' }

/**
 * The width of the semver range a specifier is saved with: `major` writes
 * `^`, `minor` writes `~`, and `patch` writes the bare version. `none` is
 * inferred from a `*` specifier and serializes like `major`, except in the
 * rolling workspace form, which keeps `*`.
 */
export type RangeSpecGranularity =
  | 'none'
  | 'patch'
  | 'minor'
  | 'major'

/**
 * How a resolved version is written back to the manifest. Beyond the
 * granularity values, `exact` selects the same single-version range as `patch`
 * but spells it with an explicit `=` operator, preserving a deliberate
 * `=x.y.z` pin. Collapse the spelling away with `@pnpm/pkg-manifest.utils`'s
 * `rangeSpecGranularity` when only the range width matters.
 */
export type RangeSpecStyle = RangeSpecGranularity | 'exact'

/** @deprecated Renamed to {@link RangeSpecStyle}. */
export type PinnedVersion = RangeSpecStyle

export type IgnoredBuilds = Set<DepPath>

export interface PackageVulnerabilityAudit {
  /**
   * Check if the given package version is vulnerable.
   */
  isVulnerable: (packageName: string, version: string) => boolean
  /**
   * Get all vulnerabilities for all packages.
   * @returns A map where the keys are package names and the values are arrays of vulnerabilities for those packages.
   */
  getVulnerabilities: () => Map<string, PackageVulnerability[]>
}

export interface PackageVulnerability {
  /**
   * A semver version range that indicates which versions are vulnerable
   */
  versionRange: string
  /**
   * The severity of the vulnerability
   */
  severity: VulnerabilitySeverity
}

export type VulnerabilitySeverity =
  | 'info'
  | 'low'
  | 'moderate'
  | 'high'
  | 'critical'
