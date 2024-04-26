import path from 'path'
import normalize from 'normalize-path'
import { type ResolvedDirectDependency, type ResolvedImporters } from './resolveDependencyTree'
import { type LinkedDependency } from './resolveDependencies'
import {
  type DependenciesByProjectId,
  type GenericDependenciesGraph,
  type PartialResolvedPackage,
  type ProjectToResolve,
} from './resolvePeers'

export interface DedupeInjectedDepsOptions<T extends PartialResolvedPackage> {
  depGraph: GenericDependenciesGraph<T>
  dependenciesByProjectId: DependenciesByProjectId
  lockfileDir: string
  pathsByNodeId: Map<string, string>
  projects: ProjectToResolve[]
  resolvedImporters: ResolvedImporters
}

export function dedupeInjectedDeps<T extends PartialResolvedPackage> (
  opts: DedupeInjectedDepsOptions<T>
): void {
  const injectedDepsByProjects = getInjectedDepsByProjects(opts)
  const dedupeMap = getDedupeMap(injectedDepsByProjects, opts)
  applyDedupeMap(dedupeMap, opts)
}

type InjectedDepsByProjects = Map<string, Map<string, { depPath: string, id: string }>>

function getInjectedDepsByProjects<T extends PartialResolvedPackage> (
  opts: Pick<DedupeInjectedDepsOptions<T>, 'projects' | 'pathsByNodeId' | 'depGraph'>
): InjectedDepsByProjects {
  const injectedDepsByProjects = new Map<string, Map<string, { depPath: string, id: string }>>()
  for (const project of opts.projects) {
    for (const [alias, nodeId] of Object.entries(project.directNodeIdsByAlias)) {
      const depPath = opts.pathsByNodeId.get(nodeId)!
      if (!opts.depGraph[depPath].id.startsWith('file:')) continue
      const id = opts.depGraph[depPath].id.substring(5)
      if (opts.projects.some((project) => project.id === id)) {
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
        .every(([alias, depPath]) => opts.dependenciesByProjectId[dep.id][alias] === depPath)
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
      delete opts.dependenciesByProjectId[id][alias]
      const index = opts.resolvedImporters[id].directDependencies.findIndex((dep) => dep.alias === alias)
      const prev = opts.resolvedImporters[id].directDependencies[index]
      const depPath = `link:${normalize(path.relative(id, dedupedProjectId))}`
      const linkedDep: LinkedDependency & ResolvedDirectDependency = {
        ...prev,
        isLinkedDependency: true,
        depPath,
        pkgId: depPath,
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
