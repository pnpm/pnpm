export interface DependencyNode {
  alias: string
  circular?: true
  deduped?: true
  /**
   * When `deduped` is true, the number of transitive dependencies that were
   * elided because this subtree was already expanded elsewhere in the tree.
   */
  dedupedDependenciesCount?: number
  /**
   * Short hash of the peer dependency suffix in the depPath, used to
   * distinguish deduped instances of the same package with different
   * peer dependency resolutions.
   */
  peersSuffixHash?: string
  dependencies?: DependencyNode[]
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
  searchMessage?: string
}
