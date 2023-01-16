export type DependenciesField = 'optionalDependencies' | 'dependencies' | 'devDependencies'

export type DependenciesOrPeersField = DependenciesField | 'peerDependencies'

// NOTE: The order in this array is important.
export const DEPENDENCIES_FIELDS: DependenciesField[] = [
  'optionalDependencies',
  'dependencies',
  'devDependencies',
]

export interface Registries {
  default: string
  [scope: string]: string
}

export type HoistedDependencies = Record<string, Record<string, 'public' | 'private'>>

export interface PatchFile {
  path: string
  hash: string
}

export interface PackageNode {
  alias: string
  circular?: true
  dependencies?: PackageNode[]
  dev?: boolean
  isPeer: boolean
  isSkipped: boolean
  isMissing: boolean
  name: string
  optional?: true
  path: string
  resolved?: string
  searched?: true
  version: string
}
