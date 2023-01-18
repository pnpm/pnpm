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
