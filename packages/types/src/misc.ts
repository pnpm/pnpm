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
