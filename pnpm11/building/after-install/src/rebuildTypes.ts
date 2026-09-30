import type { DepsGraph, DepsStateCache } from '@pnpm/deps.graph-hasher'
import type { PnpmContext } from '@pnpm/installing.context'
import type {
  LockfileObject,
  nameVerFromPkgSnapshot,
  PackageSnapshot,
  PackageSnapshots,
} from '@pnpm/lockfile.utils'
import type { ImmutableStoreIndex, StoreIndex } from '@pnpm/store.index'
import type {
  DepPath,
  IgnoredBuilds,
  ProjectId,
  ProjectRootDir,
} from '@pnpm/types'

import type { StrictBuildOptions } from './extendBuildOptions.js'

export type RebuildPackagesContext = {
  pkgsToRebuild: Set<string>
  skipped: Set<string>
  virtualStoreDir: string
  rootModulesDir: string
  currentLockfile: LockfileObject
  projects: Record<string, { id: ProjectId, rootDir: ProjectRootDir }>
  extraBinPaths: string[]
  extraNodePaths: string[]
} & Pick<PnpmContext, 'modulesFile'>

export interface RebuildPackagesResult {
  pkgsThatWereRebuilt: Set<string>
  ignoredPkgs: IgnoredBuilds
}

/** Everything one rebuild shares across the packages it builds. */
export interface RebuildState {
  ctx: RebuildPackagesContext
  opts: StrictBuildOptions
  depGraph: DepsGraph<DepPath>
  depsStateCache: DepsStateCache
  nodeVersion: string | undefined
  pkgSnapshots: PackageSnapshots
  /** Records a package the build policy says nothing about in `ignoredPkgs`. */
  allowBuild: (depPath: DepPath) => boolean
  ignoredPkgs: Set<DepPath>
  pkgsThatWereRebuilt: Set<string>
  builtDepPaths: Set<string>
  storeIndex: ImmutableStoreIndex | StoreIndex | undefined
  gvsDirByDepPath: Map<DepPath, string>
  warn: (message: string) => void
}

export interface PackageToBuild {
  depPath: DepPath
  pkgSnapshot: PackageSnapshot
  pkgInfo: ReturnType<typeof nameVerFromPkgSnapshot>
  pkgRoot: string
  gvsDir: string | undefined
}
