import { computeStronglyConnectedComponents } from '@pnpm/deps.graph-sequencer'
import type { DepPath } from '@pnpm/types'

import { type CompatibilityGraph, isCompatibleAndHasMoreDeps, nodeDepsCount } from './depPathCompatibility.js'
import type {
  GenericDependenciesGraphWithResolvedChildren,
  PartialResolvedPackage,
} from './peersResolutionTypes.js'

type DepPathSorter = (depPath1: DepPath, depPath2: DepPath) => number

export function deduplicateAll<Pkg extends PartialResolvedPackage> (
  graph: CompatibilityGraph<Pkg>,
  duplicates: Array<Set<DepPath>>
): Record<DepPath, DepPath> {
  const { depGraph } = graph
  const incoming = incomingEdges(depGraph)
  let remaining = duplicates
  const merged: Record<DepPath, DepPath> = {}
  while (true) {
    const { depPathsMap, remainingDuplicates } = deduplicateRound(remaining, graph)
    if (Object.keys(depPathsMap).length === 0) return merged
    rewriteChildren(depGraph, depPathsMap, incoming)
    transferUnboundOptionalPeers(graph, depPathsMap)
    for (const [collapsed, target] of Object.entries<DepPath>(merged)) {
      merged[collapsed as DepPath] = depPathsMap[target] ?? target
    }
    Object.assign(merged, depPathsMap)
    remaining = remainingDuplicates
  }
}

function deduplicateRound<Pkg extends PartialResolvedPackage> (
  duplicates: Array<Set<DepPath>>,
  graph: CompatibilityGraph<Pkg>
): DeduplicateDepPathsResult {
  const result = deduplicateDepPaths(duplicates, graph)
  if (Object.keys(result.depPathsMap).length > 0) return result
  const depPathsMap = collapseCycles(graph, duplicates)
  return {
    depPathsMap,
    remainingDuplicates: result.remainingDuplicates.map(group => new Set([...group].filter(path => !depPathsMap[path]))),
  }
}

interface CycleComponent {
  groups: Array<Set<DepPath>>
  dependencies: Set<number>
  cyclic: boolean
}

// Break one pair per independent cycle, deferring parents of changed components
// until their children have settled in the next ordinary round.
function collapseCycles<Pkg extends PartialResolvedPackage> (
  graph: CompatibilityGraph<Pkg>,
  duplicates: Array<Set<DepPath>>
): Record<DepPath, DepPath> {
  const components = cycleComponents(graph.depGraph, duplicates)
  const changed = components.map(() => false)
  const replacements: Record<DepPath, DepPath> = {}
  for (const [index, component] of components.entries()) {
    if ([...component.dependencies].some(dependency => changed[dependency])) {
      changed[index] = true
      continue
    }
    if (!component.cyclic) continue
    const pair = compatibleComponentPair(graph, component)
    if (pair) {
      replacements[pair[0]] = pair[1]
      changed[index] = true
    }
  }
  return replacements
}

function compatibleComponentPair<Pkg extends PartialResolvedPackage> (
  graph: CompatibilityGraph<Pkg>,
  component: CycleComponent
): [DepPath, DepPath] | undefined {
  for (const group of component.groups) {
    const pair = compatiblePair(graph, group)
    if (pair) return pair
  }
  return undefined
}

function compatiblePair<Pkg extends PartialResolvedPackage> (
  graph: CompatibilityGraph<Pkg>,
  group: Set<DepPath>
): [DepPath, DepPath] | undefined {
  const paths = [...group].sort(createDepCountSorter(graph.depGraph))
  while (paths.length) {
    const largest = paths.pop()!
    const candidate = [...paths].reverse().find(path => isCompatibleAndHasMoreDeps(graph, largest, path))
    if (candidate) return [candidate, largest]
  }
  return undefined
}

function cycleComponents<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  duplicates: Array<Set<DepPath>>
): CycleComponent[] {
  const groups = duplicates.filter(group => group.size > 1).sort((left, right) => {
    const leftId = depGraph[left.values().next().value!].pkgIdWithPatchHash
    const rightId = depGraph[right.values().next().value!].pkgIdWithPatchHash
    return leftId < rightId ? -1 : leftId > rightId ? 1 : 0
  })
  const byId = new Map(groups.map((group, index) => [depGraph[group.values().next().value!].pkgIdWithPatchHash, index]))
  const adjacency = groups.map(group => groupDependencies(depGraph, group, byId))
  const analysis = computeStronglyConnectedComponents(adjacency, adjacency.map(() => false))
  const components: CycleComponent[] = analysis.componentSize.map(size => ({ groups: [], dependencies: new Set(), cyclic: size > 1 }))
  populateComponents({ groups, adjacency, componentOf: analysis.componentOf, components })
  return components
}

function populateComponents ({ groups, adjacency, componentOf, components }: {
  groups: Array<Set<DepPath>>
  adjacency: number[][]
  componentOf: number[]
  components: CycleComponent[]
}): void {
  for (const [index, group] of groups.entries()) {
    const component = components[componentOf[index]]
    component.groups.push(group)
    if (adjacency[index].includes(index)) component.cyclic = true
    for (const child of adjacency[index]) {
      if (componentOf[child] !== componentOf[index]) component.dependencies.add(componentOf[child])
    }
  }
}

function groupDependencies<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  group: Set<DepPath>,
  byId: Map<Pkg['pkgIdWithPatchHash'], number>
): number[] {
  const dependencies = new Set<number>()
  for (const variant of group) {
    for (const child of Object.values(depGraph[variant].children)) {
      const node = depGraph[child]
      const index = node && byId.get(node.pkgIdWithPatchHash)
      if (index != null) dependencies.add(index)
    }
  }
  return [...dependencies].sort((left, right) => left - right)
}

// A variant that absorbed a protected one inherits its protection, so a later
// round cannot collapse it into a variant that resolves the protected peer.
function transferUnboundOptionalPeers<Pkg extends PartialResolvedPackage> (
  { unboundOptionalPeers }: CompatibilityGraph<Pkg>,
  depPathsMap: Record<DepPath, DepPath>
): void {
  if (!unboundOptionalPeers?.size) return
  for (const [collapsed, target] of Object.entries<DepPath>(depPathsMap)) {
    const names = unboundOptionalPeers.get(collapsed as DepPath)
    if (names == null) continue
    unboundOptionalPeers.set(target, new Set([...unboundOptionalPeers.get(target) ?? [], ...names]))
  }
}

type IncomingEdges = Map<DepPath, Array<[DepPath, string]>>

function incomingEdges<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>
): IncomingEdges {
  const incoming: IncomingEdges = new Map()
  for (const node of Object.values(depGraph)) {
    for (const [alias, child] of Object.entries<DepPath>(node.children)) {
      const edges = incoming.get(child) ?? []
      edges.push([node.depPath, alias])
      incoming.set(child, edges)
    }
  }
  return incoming
}

function rewriteChildren<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  depPathsMap: Record<DepPath, DepPath>,
  incoming: IncomingEdges
): void {
  for (const [collapsed, target] of Object.entries<DepPath>(depPathsMap)) {
    const edges = incoming.get(collapsed as DepPath) ?? []
    incoming.delete(collapsed as DepPath)
    const targetEdges = incoming.get(target) ?? []
    for (const edge of edges) {
      const [parent, alias] = edge
      depGraph[parent].children[alias] = target
      targetEdges.push(edge)
    }
    incoming.set(target, targetEdges)
  }
}

interface DeduplicateDepPathsResult {
  depPathsMap: Record<DepPath, DepPath>
  remainingDuplicates: Array<Set<DepPath>>
}

function deduplicateDepPaths<Pkg extends PartialResolvedPackage> (
  duplicates: Array<Set<DepPath>>,
  graph: CompatibilityGraph<Pkg>
): DeduplicateDepPathsResult {
  const depCountSorter = createDepCountSorter(graph.depGraph)
  const depPathsMap: Record<DepPath, DepPath> = {}
  const remainingDuplicates: Array<Set<DepPath>> = []

  for (const depPaths of duplicates) {
    const survivingDepPaths = collapseCompatibleDepPaths(graph, { depPaths, depPathsMap, depCountSorter })
    if (survivingDepPaths.size > 1) {
      remainingDuplicates.push(survivingDepPaths)
    }
  }
  return {
    depPathsMap,
    remainingDuplicates,
  }
}

// The dep paths arrive in resolution order, which varies between platforms.
// Tie-break equal dependency counts on the dep path itself so the chosen
// collapse target is the same everywhere — when a variant is a subset of
// several incompatible larger variants, the winner must not hinge on
// iteration order, or the lockfile becomes machine-dependent.
function createDepCountSorter<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>
): DepPathSorter {
  return (depPath1: DepPath, depPath2: DepPath) => {
    const countDiff = nodeDepsCount(depGraph[depPath1]) - nodeDepsCount(depGraph[depPath2])
    if (countDiff !== 0) return countDiff
    return depPath1 < depPath2 ? -1 : depPath1 > depPath2 ? 1 : 0
  }
}

/**
 * Maps every dep path of `depPaths` that a compatible variant with more
 * dependencies can replace into `depPathsMap`, and returns the dep paths
 * that survive the round.
 */
function collapseCompatibleDepPaths<Pkg extends PartialResolvedPackage> (
  graph: CompatibilityGraph<Pkg>,
  opts: {
    depPaths: Set<DepPath>
    depPathsMap: Record<DepPath, DepPath>
    depCountSorter: DepPathSorter
  }
): Set<DepPath> {
  const { depGraph } = graph
  const survivingDepPaths = new Set(opts.depPaths.values())
  let currentDepPaths = [...opts.depPaths].sort(opts.depCountSorter)

  while (currentDepPaths.length) {
    const depPath1 = currentDepPaths.pop()!
    const nextDepPaths = []
    while (currentDepPaths.length) {
      const depPath2 = currentDepPaths.pop()!
      // Child variants must agree before a parent can collapse into its sibling.
      const sameChildren = Object.entries(depGraph[depPath2].children).every(
        ([alias, child]) => depGraph[depPath1].children[alias] === child
      )
      if (sameChildren && isCompatibleAndHasMoreDeps(graph, depPath1, depPath2)) {
        opts.depPathsMap[depPath2] = depPath1
        survivingDepPaths.delete(depPath2)
      } else {
        nextDepPaths.push(depPath2)
      }
    }
    nextDepPaths.push(...currentDepPaths)
    currentDepPaths = nextDepPaths.sort(opts.depCountSorter)
  }
  return survivingDepPaths
}
