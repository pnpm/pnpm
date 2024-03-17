import type { DependenciesHierarchy } from '@pnpm/reviewing.dependencies-hierarchy'

export type PackageDependencyHierarchy = DependenciesHierarchy & {
  name?: string | undefined
  version?: string | undefined
  path: string
  private?: boolean | undefined
}
