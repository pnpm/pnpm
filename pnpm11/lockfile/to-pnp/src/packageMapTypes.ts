import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath } from '@pnpm/types'

export interface PackageMap {
  packages: Record<string, PackageMapPackage>
}

export interface PackageMapPackage {
  url: string
  dependencies: Record<string, string>
}

export type PackageMapType = 'standard' | 'loose'

export interface PackageMapOptions {
  importerNames: Record<string, string | undefined>
  lockfileDir: string
  packageMapType?: PackageMapType
  rootModulesDir: string
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  /**
   * Real on-disk directory of each package, keyed by depPath. Required for the
   * global virtual store, where packages live at a content-hashed path that
   * cannot be derived from the depPath alone. Falls back to the local
   * `<virtualStoreDir>/<depPathToFilename>` layout when a depPath is absent.
   */
  locationByDepPath?: Record<string, string>
}

export interface DependenciesGraphPackageMapOptions {
  directDependenciesByImporterId: Record<string, Record<string, string>>
  graph: Record<string, PackageMapGraphNode>
  importerNames: Record<string, string | undefined>
  lockfile: LockfileObject
  lockfileDir: string
  packageMapType?: PackageMapType
  rootModulesDir: string
  packageIdStrategy: 'depPath' | 'path'
}

export interface PackageMapGraphNode {
  children: Record<string, string>
  depPath: DepPath
  dir: string
  name: string
}
