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

/** Parsed value of `_auth` of each registry in the rc file. */
export interface BasicAuth {
  username: string
  password: string
}

/** Parsed value of `tokenHelper` of each registry in the rc file. */
export type TokenHelper = [string, ...string[]]

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
  creds?: Creds
  tls?: TlsConfig
}

export type HoistedDependencies = Record<DepPath | ProjectId, Record<string, 'public' | 'private'>>

export type PkgResolutionId = string & { __brand: 'PkgResolutionId' }

export type PkgId = string & { __brand: 'PkgId' }

export type PkgIdWithPatchHash = string & { __brand: 'PkgIdWithPatchHash' }

export type DepPath = string & { __brand: 'DepPath' }

export type ProjectId = string & { __brand: 'ProjectId' }

export type PinnedVersion =
  | 'none'
  | 'patch'
  | 'minor'
  | 'major'

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
