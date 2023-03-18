import { type DependenciesHierarchy } from '@pnpm/reviewing.dependencies-hierarchy'

export type PackageDependencyHierarchy = DependenciesHierarchy & {
  name?: string
  version?: string
  path: string
  private?: boolean
}
