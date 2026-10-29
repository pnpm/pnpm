import { hashObject } from '@pnpm/crypto.object-hasher'
import { packageRootLinkTarget } from '@pnpm/deps.path'
import type { SupportedArchitectures } from '@pnpm/types'

import { createFullPkgId } from './lockfileToDepGraph.js'
import type { DepGraphHashContext, DepsGraph, DepsGraphNode, DepsStateCache } from './types.js'

export interface CalcDepGraphHashOptions<NodeId extends string> {
  depsGraph: DepsGraph<NodeId>
  cache: DepsStateCache
  /** @deprecated Ignored. Dependency cycles are detected from the graph. */
  parents?: Set<string>
  depPath: NodeId
  context: DepGraphHashContext
}

/**
 * The digest of the subgraph depPath reaches.
 *
 * A node outside any dependency cycle hashes as `hashObject({ id, deps })`,
 * where deps maps each alias to the child's digest. A child the graph has no
 * node for contributes ''.
 *
 * The members of a cycle (a strongly connected component, or a node that
 * depends on itself) hash as one unit, so each member's digest covers
 * everything the cycle reaches. See {@link hashCycle}.
 *
 * The digests depend only on the graph, not on the order nodes or children
 * are visited in, so a shared cache gives the same answers whichever root
 * reaches a node first.
 */
export function calcDepGraphHash<NodeId extends string> ({
  depsGraph,
  cache,
  depPath,
  context,
}: CalcDepGraphHashOptions<NodeId>): string {
  const cacheKey = `${context.cacheKeyPrefix}${depPath}`
  if (cache[cacheKey]) return cache[cacheKey]
  if (!depsGraph[depPath]) return ''
  visit({
    depsGraph,
    cache,
    context,
    visits: new Map(),
    stack: [],
  }, depPath)
  return cache[cacheKey]
}

/**
 * Tarjan's strongly-connected-components search over the nodes that are not
 * in the cache yet. It finishes a component only after every component the
 * component depends on, so a component's external children are always hashed
 * by the time it is.
 */
interface ComponentWalk<NodeId extends string> {
  depsGraph: DepsGraph<NodeId>
  cache: DepsStateCache
  context: DepGraphHashContext
  visits: Map<NodeId, Visit>
  stack: NodeId[]
}

interface Visit {
  index: number
  lowLink: number
  onStack: boolean
}

function visit<NodeId extends string> (walk: ComponentWalk<NodeId>, depPath: NodeId): void {
  const index = walk.visits.size
  const ownVisit: Visit = { index, lowLink: index, onStack: true }
  walk.visits.set(depPath, ownVisit)
  walk.stack.push(depPath)
  for (const childId of hashedChildIds(walk.depsGraph[depPath])) {
    if (!walk.depsGraph[childId] || walk.cache[`${walk.context.cacheKeyPrefix}${childId}`]) continue
    const childVisit = walk.visits.get(childId)
    if (childVisit == null) {
      visit(walk, childId)
      ownVisit.lowLink = Math.min(ownVisit.lowLink, walk.visits.get(childId)!.lowLink)
    } else if (childVisit.onStack) {
      ownVisit.lowLink = Math.min(ownVisit.lowLink, childVisit.index)
    }
  }
  if (ownVisit.lowLink !== index) return
  const members = walk.stack.splice(walk.stack.lastIndexOf(depPath))
  for (const member of members) {
    walk.visits.get(member)!.onStack = false
  }
  hashComponent(walk, members)
}

function hashComponent<NodeId extends string> (walk: ComponentWalk<NodeId>, members: NodeId[]): void {
  const { depsGraph, cache, context } = walk
  const cacheKey = (depPath: NodeId) => `${context.cacheKeyPrefix}${depPath}`
  if (members.length === 1) {
    const depPath = members[0]
    const node = depsGraph[depPath]
    if (!hashedChildIds(node).includes(depPath)) {
      cache[cacheKey(depPath)] = hashObject({
        id: resolveNodeFullPkgId(depPath, node, context.supportedArchitectures),
        deps: mapChildren(node, () => true, (childId) => cache[cacheKey(childId)] ?? ''),
      })
      return
    }
  }
  const hashes = hashCycle(walk, members)
  members.forEach((member, position) => {
    cache[cacheKey(member)] = hashes[position]
  })
}

/**
 * The digests of the members of one dependency cycle, in members order.
 * Every child outside the cycle must already be in the cache.
 *
 * Each member is first labelled `hashObject({ id, deps })` over its children
 * outside the cycle. The labels are then refined with the children inside
 * the cycle, `hashObject({ color: <label>, deps: { <alias>: <child label> } })`,
 * until a round no longer splits a group of equal labels. Two members end
 * with the same label only when their dependency trees are the same, however
 * far they are unrolled.
 *
 * The cycle's shape maps each final label to that member's
 * `{ <alias>: <child label> }` inside the cycle. A member's digest is
 * `hashObject({ member: <label>, component: hashObject(shape) })`, so it
 * changes when anything in or below the cycle changes.
 */
function hashCycle<NodeId extends string> (
  { depsGraph, cache, context }: ComponentWalk<NodeId>,
  members: NodeId[]
): string[] {
  const position = new Map(members.map((member, index) => [member, index]))
  const isInternal = (childId: NodeId) => position.has(childId)
  const internalDeps = (member: NodeId, labels: string[]) =>
    mapChildren(depsGraph[member], isInternal, (childId) => labels[position.get(childId)!])
  const labels = refineLabels(members.map((member) => hashObject({
    id: resolveNodeFullPkgId(member, depsGraph[member], context.supportedArchitectures),
    deps: mapChildren(
      depsGraph[member],
      (childId) => !isInternal(childId),
      (childId) => cache[`${context.cacheKeyPrefix}${childId}`] ?? ''
    ),
  })), (labels, index) => internalDeps(members[index], labels))
  const shape: Record<string, Record<string, string>> = {}
  members.forEach((member, index) => {
    shape[labels[index]] = internalDeps(member, labels)
  })
  const component = hashObject(shape)
  return labels.map((label) => hashObject({ member: label, component }))
}

/**
 * Refine each member's label with its children's labels until a round no
 * longer splits a group of equal labels.
 */
function refineLabels (
  labels: string[],
  internalDeps: (labels: string[], index: number) => Record<string, string>
): string[] {
  const groups = new Set(labels).size
  const refined = labels.map((label, index) => hashObject({
    color: label,
    deps: internalDeps(labels, index),
  }))
  return new Set(refined).size === groups ? labels : refineLabels(refined, internalDeps)
}

function hashedChildIds<NodeId extends string> (node: DepsGraphNode<NodeId>): NodeId[] {
  return Object.values(node.children).filter((childId) => packageRootLinkTarget(childId) == null)
}

function mapChildren<NodeId extends string> (
  node: DepsGraphNode<NodeId>,
  include: (childId: NodeId) => boolean,
  valueOf: (childId: NodeId) => string
): Record<string, string> {
  const deps: Record<string, string> = {}
  for (const [alias, childId] of Object.entries(node.children) as Array<[string, NodeId]>) {
    if (packageRootLinkTarget(childId) != null || !include(childId)) continue
    deps[alias] = valueOf(childId)
  }
  return deps
}

function resolveNodeFullPkgId<NodeId extends string> (
  depPath: NodeId,
  node: DepsGraphNode<NodeId>,
  supportedArchitectures?: SupportedArchitectures
): string {
  if (node.pkgIdWithPatchHash != null && node.resolution != null) {
    return createFullPkgId(node.pkgIdWithPatchHash, node.resolution, supportedArchitectures)
  }
  if (node.fullPkgId != null) {
    return node.fullPkgId
  }
  throw new Error(`fullPkgId or resolution metadata is not defined for ${depPath} in depsGraph`)
}

export function createDepGraphHashContext (supportedArchitectures?: SupportedArchitectures): DepGraphHashContext {
  return {
    supportedArchitectures,
    cacheKeyPrefix: supportedArchitectures == null ? '' : `\0architectures=${hashObject(supportedArchitectures)}\0`,
  }
}
