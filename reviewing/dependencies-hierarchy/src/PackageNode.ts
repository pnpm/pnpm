export interface PackageNode {
  alias: string
  circular?: true
  deduped?: true
  /**
   * When `deduped` is true, the number of transitive dependencies that were
   * elided because this subtree was already expanded elsewhere in the tree.
   */
  dedupedDependenciesCount?: number
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
  searchMessage?: string
}
