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
 * Registry aliases a dependency specifier can name (`work:foo`), with the
 * built-ins guaranteed present.
 *
 * The required keys are the enforcement: a raw `namedRegistries` straight out
 * of the user's config does not satisfy this type, so it cannot reach a
 * consumer without passing through `normalizeNamedRegistries` first. Without
 * that, forgetting to fill in the built-ins somewhere would silently stop
 * `gh:` and `npmjs:` from resolving on that code path.
 *
 * Adding a built-in means widening this type, which is deliberate: it is the
 * same acknowledgement the reverse-routing prefix test asks for, since these
 * URLs also decide where verification traffic goes.
 */
export interface NamedRegistries {
  gh: string
  npmjs: string
  [alias: string]: string
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
