import type { DependenciesTree } from '@pnpm/deps.inspection.dependencies-hierarchy'

export interface PackageDependencyHierarchy extends DependenciesTree {
  name?: string
  version?: string
  path: string
  private?: boolean
}
