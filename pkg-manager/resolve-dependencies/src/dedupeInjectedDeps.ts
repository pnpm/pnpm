import path from 'path'
import normalize from 'normalize-path'
import { type PkgResolutionId, type DepPath } from '@pnpm/types'
import { type ResolvedDirectDependency, type ResolvedImporters } from './resolveDependencyTree.js'
import { type NodeId } from './nextNodeId.js'
import { type LinkedDependency } from './resolveDependencies.js'
import {
  type GenericDependenciesGraphWithResolvedChildren,
  type DependenciesByProjectId,
  type PartialResolvedPackage,
  type ProjectToResolve,
} from './resolvePeers.js'

export interface DedupeInjectedDepsOptions<T extends PartialResolvedPackage> {
  depGraph: GenericDependenciesGraphWithResolvedChildren<T>
  dependenciesByProjectId: DependenciesByProjectId
  lockfileDir: string
  pathsByNodeId: Map<NodeId, DepPath>
  projects: ProjectToResolve[]
  resolvedImporters: ResolvedImporters
  workspacePackages: string[]
}

export function dedupeInjectedDeps<T extends PartialResolvedPackage> (
  opts: DedupeInjectedDepsOptions<T>
): void {
  const injectedDepsByProjects = getInjectedDepsByProjects(opts)
  const dedupeMap = getDedupeMap(injectedDepsByProjects, opts)
  applyDedupeMap(dedupeMap, opts)
}

type InjectedDepsByProjects = Map<string, Map<string, { depPath: DepPath, id: string }>>

function getInjectedDepsByProjects<T extends PartialResolvedPackage> (
  opts: Pick<DedupeInjectedDepsOptions<T>, 'projects' | 'pathsByNodeId' | 'depGraph' | 'workspacePackages'>
): InjectedDepsByProjects {
  const injectedDepsByProjects = new Map<string, Map<string, { depPath: DepPath, id: string }>>()
  for (const project of opts.projects) {
    for (const [alias, nodeId] of project.directNodeIdsByAlias.entries()) {
      const depPath = opts.pathsByNodeId.get(nodeId)!
      if (!opts.depGraph[depPath].id.startsWith('file:')) continue
      const id = opts.depGraph[depPath].id.substring(5)
      if (opts.workspacePackages.includes(id)) {
        if (!injectedDepsByProjects.has(project.id)) injectedDepsByProjects.set(project.id, new Map())
        injectedDepsByProjects.get(project.id)!.set(alias, { depPath, id })
      }
    }
  }
  return injectedDepsByProjects
}

type DedupeMap = Map<string, Map<string, string>>

function getDedupeMap<T extends PartialResolvedPackage> (
  injectedDepsByProjects: InjectedDepsByProjects,
  opts: Pick<DedupeInjectedDepsOptions<T>, 'depGraph' | 'dependenciesByProjectId'>
): DedupeMap {
  const toDedupe = new Map<string, Map<string, string>>()
  for (const [id, deps] of injectedDepsByProjects.entries()) {
    const dedupedInjectedDeps = new Map<string, string>()
    for (const [alias, dep] of deps.entries()) {
      // Check for subgroup not equal.
      // The injected project in the workspace may have dev deps
      const isSubset = Object.entries(opts.depGraph[dep.depPath].children)
        .every(([alias, depPath]) => opts.dependenciesByProjectId[dep.id].get(alias) === depPath)
      if (isSubset) {
        dedupedInjectedDeps.set(alias, dep.id)
      }
    }
    toDedupe.set(id, dedupedInjectedDeps)
  }
  return toDedupe
}

function applyDedupeMap<T extends PartialResolvedPackage> (
  dedupeMap: DedupeMap,
  opts: Pick<DedupeInjectedDepsOptions<T>, 'dependenciesByProjectId' | 'resolvedImporters' | 'lockfileDir'>
): void {
  for (const [id, aliases] of dedupeMap.entries()) {
    for (const [alias, dedupedProjectId] of aliases.entries()) {
      opts.dependenciesByProjectId[id].delete(alias)
      const index = opts.resolvedImporters[id].directDependencies.findIndex((dep) => dep.alias === alias)
      const prev = opts.resolvedImporters[id].directDependencies[index]
      const linkedDep: LinkedDependency & ResolvedDirectDependency = {
        ...prev,
        pkg: prev,
        isLinkedDependency: true,
        pkgId: `link:${normalize(path.relative(id, dedupedProjectId))}` as PkgResolutionId,
        resolution: {
          type: 'directory',
          directory: path.join(opts.lockfileDir, dedupedProjectId),
        },
      }
      opts.resolvedImporters[id].directDependencies[index] = linkedDep
      opts.resolvedImporters[id].linkedDependencies.push(linkedDep)
    }
  }
}
