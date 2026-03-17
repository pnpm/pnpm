import type { DependenciesTree } from '@pnpm/deps.inspection.tree-builder'

export interface PackageDependencyHierarchy extends DependenciesTree {
  name?: string
  version?: string
  path: string
  private?: boolean
}
