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

export interface SslConfig {
  cert: string
  key: string
  ca?: string
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
  | 'low'
  | 'moderate'
  | 'high'
  | 'critical'
