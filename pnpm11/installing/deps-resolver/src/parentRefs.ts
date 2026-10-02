import semver from 'semver'

import type { NodeId } from './nextNodeId.js'
import type {
  ParentPkgNode,
  ParentRef,
  ParentRefs,
  PartialResolvedPackage,
} from './peersResolutionTypes.js'
import type { DependenciesTree } from './resolveDependencies.js'

export function createPkgsByName<Pkg extends PartialResolvedPackage> (
  dependenciesTree: DependenciesTree<Pkg>,
  { directNodeIdsByAlias, topParents }: {
    directNodeIdsByAlias: Map<string, NodeId>
    topParents: Array<{ name: string, version: string, alias?: string, linkedDir?: string }>
  }
): ParentRefs {
  const parentRefs = toPkgByName(
    Array.from(directNodeIdsByAlias.entries())
      .map(([alias, nodeId]) => ({
        alias,
        node: dependenciesTree.get(nodeId)!,
        nodeId,
        parentNodeIds: [],
      }))
  )
  const _updateParentRefs = updateParentRefs.bind(null, parentRefs)
  for (const { name, version, alias, linkedDir } of topParents) {
    const pkg = {
      occurrence: 0,
      alias,
      depth: 0,
      version,
      nodeId: linkedDir as NodeId,
      parentNodeIds: [],
    }
    _updateParentRefs(name, pkg)
    if (alias && alias !== name) {
      _updateParentRefs(alias, pkg)
    }
  }
  return parentRefs
}

export function toPkgByName<Pkg extends PartialResolvedPackage> (nodes: Array<ParentPkgNode<Pkg>>): ParentRefs {
  const pkgsByName: ParentRefs = {}
  const _updateParentRefs = updateParentRefs.bind(null, pkgsByName)
  for (const { alias, node, nodeId, parentNodeIds } of nodes) {
    const pkg = {
      alias,
      depth: node.depth,
      nodeId,
      version: node.resolvedPackage.version,
      occurrence: 0,
      parentNodeIds,
    }
    _updateParentRefs(alias, pkg)
    if (alias !== node.resolvedPackage.name) {
      _updateParentRefs(node.resolvedPackage.name, pkg)
    }
  }
  return pkgsByName
}

function updateParentRefs (parentRefs: ParentRefs, newAlias: string, pkg: ParentRef): void {
  const existing = parentRefs[newAlias]
  if (existing) {
    const existingHasAlias = existing.alias != null && existing.alias !== newAlias
    if (!existingHasAlias) return
    const newHasAlias = pkg.alias != null && pkg.alias !== newAlias
    if (newHasAlias && semver.gte(existing.version, pkg.version)) return
  }
  parentRefs[newAlias] = pkg
}

export function parentPkgsMatch<Pkg> (
  dependenciesTree: DependenciesTree<Pkg>,
  currentParentPkg: ParentRef,
  newParentPkg: ParentRef
): boolean {
  if (
    currentParentPkg.version !== newParentPkg.version ||
    currentParentPkg.alias !== newParentPkg.alias
  ) {
    return false
  }
  const currentParentResolvedPkg = currentParentPkg.nodeId && dependenciesTree.get(currentParentPkg.nodeId)?.resolvedPackage
  if (currentParentResolvedPkg == null) return true
  const newParentResolvedPkg = newParentPkg.nodeId && dependenciesTree.get(newParentPkg.nodeId)?.resolvedPackage
  if (newParentResolvedPkg == null) return true
  return currentParentResolvedPkg.name === newParentResolvedPkg.name
}
