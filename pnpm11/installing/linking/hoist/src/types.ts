import type { DepPath, HoistedDependencies } from '@pnpm/types'

export interface DependenciesGraphNode<NodeId extends string> {
  dir: string
  children: Record<string, NodeId>
  optionalDependencies: Set<string>
  hasBin: boolean
  name: string
  depPath: DepPath
}

export type DependenciesGraph<NodeId extends string> = Record<NodeId, DependenciesGraphNode<NodeId>>

export interface DirectDependenciesByImporterId<NodeId extends string> {
  [importerId: string]: Map<string, NodeId>
}

export interface HoistedWorkspaceProject {
  name: string
  dir: string
}

export type HoistType = 'private' | 'public'

export type HoistedDependenciesByNodeId<NodeId extends string> = Map<NodeId, Record<string, HoistType>>

export interface HoistGraphResult<NodeId extends string> {
  hoistedDependencies: HoistedDependencies
  hoistedDependenciesByNodeId: HoistedDependenciesByNodeId<NodeId>
  hoistedAliasesWithBins: string[]
}

export interface HoistedModulesDirs {
  privateHoistedModulesDir: string
  publicHoistedModulesDir: string
}
