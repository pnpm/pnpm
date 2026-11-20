import type { LockfileObject, PackageSnapshots } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DepPath } from '@pnpm/types'

import type { DependencyEdge } from './auditIndexTypes.js'
import { snapshotChildren, type SnapshotChildrenOptions } from './dependencyEdges.js'

interface TarjanState {
  packages: PackageSnapshots
  vulnerableNames: Set<string>
  childOpts: SnapshotChildrenOptions
  // Final reachable set per node, shared across its SCC.
  memo: Map<DepPath, Set<string>>
  // Per-node contribution, merged into the shared set when the SCC closes.
  partial: Map<DepPath, Set<string>>
  index: Map<DepPath, number>
  lowlink: Map<DepPath, number>
  onStack: Set<DepPath>
  sccStack: DepPath[]
  counter: number
}

interface SccFrame {
  edge: DependencyEdge
  own: Set<string>
  children: DependencyEdge[]
  next: number
}

// For each node, the set of vulnerabilities reachable from it (itself included),
// used by the walker to prune subtrees that reach no unsaturated finding.
// Tarjan's SCC algorithm scans every node once and shares one set across a
// cycle, avoiding the quadratic recompute of memoizing only acyclic subtrees.
export function createReachableVulnerabilitiesGetter (
  lockfile: LockfileObject,
  vulnerableNames: Set<string>,
  childOpts: SnapshotChildrenOptions
): (edge: DependencyEdge) => ReadonlySet<string> {
  const tarjan: TarjanState = {
    packages: lockfile.packages ?? {},
    vulnerableNames,
    childOpts,
    memo: new Map(),
    partial: new Map(),
    index: new Map(),
    lowlink: new Map(),
    onStack: new Set(),
    sccStack: [],
    counter: 0,
  }

  return (edge) => {
    if (!tarjan.index.has(edge.depPath)) buildScc(tarjan, edge)
    // strongconnect always finalizes the queried node's SCC, so its reachable
    // set is present afterwards. A missing entry would be a bug that silently
    // under-reports and hides a real finding, so fail loudly instead of
    // returning an empty set.
    const reachable = tarjan.memo.get(edge.depPath)
    if (reachable == null) {
      throw new Error(`Reachable vulnerabilities were not computed for ${edge.depPath}`)
    }
    return reachable
  }
}

// Iterative Tarjan: an explicit frame stack stands in for the recursive
// strongconnect, so a deep dependency chain from an untrusted lockfile cannot
// overflow the call stack. Each frame's `own` set is shared by reference with
// `partial`, so contributions merged in while the children run are visible
// when the SCC closes.
function buildScc (tarjan: TarjanState, rootEdge: DependencyEdge): void {
  const work: SccFrame[] = [openSccFrame(tarjan, rootEdge)]
  while (work.length > 0) {
    const frame = work[work.length - 1]
    if (frame.next < frame.children.length) {
      const child = frame.children[frame.next++]
      if (!tarjan.index.has(child.depPath)) {
        work.push(openSccFrame(tarjan, child))
        continue
      }
      foldVisitedChild(tarjan, frame, child.depPath)
      continue
    }

    closeSccIfRoot(tarjan, frame.edge.depPath)
    work.pop()
    const parent = work[work.length - 1]
    if (parent != null) {
      foldFinishedChild(tarjan, parent, frame.edge.depPath)
    }
  }
}

function openSccFrame (tarjan: TarjanState, edge: DependencyEdge): SccFrame {
  tarjan.index.set(edge.depPath, tarjan.counter)
  tarjan.lowlink.set(edge.depPath, tarjan.counter)
  tarjan.counter++
  tarjan.sccStack.push(edge.depPath)
  tarjan.onStack.add(edge.depPath)

  const pkgSnapshot = tarjan.packages[edge.depPath]
  const own = new Set<string>()
  let children: DependencyEdge[] = []
  if (pkgSnapshot != null) {
    const { name, version } = nameVerFromPkgSnapshot(edge.depPath, pkgSnapshot)
    const resolvedName = name ?? edge.name
    if (version && tarjan.vulnerableNames.has(resolvedName)) {
      own.add(vulnerabilityKey(resolvedName, version, edge.depPath))
    }
    children = snapshotChildren({ depPath: edge.depPath, snapshot: pkgSnapshot }, tarjan.childOpts)
  }
  tarjan.partial.set(edge.depPath, own)
  return { edge, own, children, next: 0 }
}

function foldVisitedChild (tarjan: TarjanState, frame: SccFrame, childDepPath: DepPath): void {
  if (tarjan.onStack.has(childDepPath)) {
    lowerLowlink(tarjan, frame.edge.depPath, tarjan.index.get(childDepPath)!)
  }
  // Finalized successors are already in `memo`; same-SCC ones are folded in
  // when the SCC closes.
  const childReachable = tarjan.memo.get(childDepPath)
  if (childReachable) addAll(frame.own, childReachable)
}

// Apply the post-DFS update to the parent: propagate the child's lowlink
// and fold in its reachable set once finalized (same-SCC nodes are folded
// later, via `partial`, when the shared SCC root closes).
function foldFinishedChild (tarjan: TarjanState, parent: SccFrame, childDepPath: DepPath): void {
  lowerLowlink(tarjan, parent.edge.depPath, tarjan.lowlink.get(childDepPath)!)
  const childReachable = tarjan.memo.get(childDepPath)
  if (childReachable) addAll(parent.own, childReachable)
}

function lowerLowlink (tarjan: TarjanState, depPath: DepPath, candidate: number): void {
  tarjan.lowlink.set(depPath, Math.min(tarjan.lowlink.get(depPath)!, candidate))
}

function closeSccIfRoot (tarjan: TarjanState, rootDepPath: DepPath): void {
  if (tarjan.lowlink.get(rootDepPath) !== tarjan.index.get(rootDepPath)) return
  const members: DepPath[] = []
  // Reuse the first member's own set as the shared accumulator instead of
  // allocating a fresh one, so the common singleton-SCC case finalizes
  // without any extra Set allocation or copy.
  let shared: Set<string> | undefined
  let member: DepPath
  do {
    member = tarjan.sccStack.pop()!
    tarjan.onStack.delete(member)
    members.push(member)
    const own = tarjan.partial.get(member)!
    tarjan.partial.delete(member)
    if (shared === undefined) {
      shared = own
    } else {
      addAll(shared, own)
    }
  } while (member !== rootDepPath)
  for (const sccMember of members) {
    tarjan.memo.set(sccMember, shared!)
  }
}

function vulnerabilityKey (name: string, version: string, depPath: DepPath): string {
  return `${name}\0${version}\0${depPath}`
}

export function parseVulnerabilityKey (key: string): { name: string, version: string, depPath: DepPath } {
  const [name, version, depPath] = key.split('\0')
  return { name, version, depPath: depPath as DepPath }
}

function addAll<Item> (target: Set<Item>, source: Set<Item>): void {
  for (const value of source) {
    target.add(value)
  }
}
