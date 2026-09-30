import type { LockfileResolution } from '@pnpm/lockfile.types'
import type { DepPath, PkgIdWithPatchHash, SupportedArchitectures } from '@pnpm/types'

export type DepsGraph<NodeId extends string> = Record<NodeId, DepsGraphNode<NodeId>>

export interface DepsGraphNode<NodeId extends string> {
  children: { [alias: string]: NodeId }
  pkgIdWithPatchHash?: PkgIdWithPatchHash
  resolution?: LockfileResolution
  // The full package ID is a unique fingerprint based on the package’s
  // integrity checksum, patch information, and other resolution data.
  fullPkgId?: string
}

export interface DepsStateCache {
  [depPath: string]: string
}

export interface DepGraphHashContext {
  supportedArchitectures?: SupportedArchitectures
  cacheKeyPrefix: string
}

export interface PkgMeta {
  depPath: DepPath
  name: string
  version: string
}

export type PkgMetaIterator<Meta extends PkgMeta> = IterableIterator<Meta>

export interface HashedDepPath<Meta extends PkgMeta> {
  pkgMeta: Meta
  hash: string
}
