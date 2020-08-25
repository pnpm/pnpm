import { DependenciesHierarchy } from 'dependencies-hierarchy'

export type PackageDependencyHierarchy = DependenciesHierarchy & {
  name?: string
  version?: string
  path: string
}
