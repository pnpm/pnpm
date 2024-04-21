import { type DependenciesHierarchy } from '@pnpm/reviewing.dependencies-hierarchy'

export interface PackageDependencyHierarchy extends DependenciesHierarchy {
  name?: string
  version?: string
  path: string
  private?: boolean
}
