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

export interface Registries {
  default: string
  [scope: string]: string
}

/**
 * The required keys are the enforcement: a raw `namedRegistries` from user
 * config does not satisfy this type, so it cannot reach a consumer without
 * passing through `normalizeNamedRegistries`. Widening it is how adding a
 * built-in gets acknowledged.
 */
export interface NamedRegistries {
  gh: string
  npmjs: string
  [name: string]: string
}

/**
 * The software serving a registry, when it lays out tarball URLs differently
 * from the npm registry. `npm` is the layout every registry is assumed to use
 * unless the `registryOptions` setting says otherwise.
 *
 * Only layouts pnpm can rebuild a URL for belong here. A registry that serves
 * tarballs from an opaque path (GitHub Packages `/download/<hash>`) has no
 * entry: its URLs are kept in the lockfile instead.
 */
export type RegistryServerType = 'npm' | 'artifactory'

/**
 * Non-secret, per-registry settings from the `registryOptions` setting. Held
 * apart from {@link RegistryConfig} so the install and lockfile layers can be
 * handed a registry's layout without also being handed its credentials.
 */
export interface RegistryOptions {
  serverType?: RegistryServerType
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
