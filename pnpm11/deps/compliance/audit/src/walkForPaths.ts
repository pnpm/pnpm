import { DepType, type DepTypes } from '@pnpm/lockfile.detect-dep-types'
import { getPeerSatisfactionEdgesToSkip } from '@pnpm/lockfile.peer-edges'
import type { LockfileObject, PackageSnapshots, ProjectSnapshot } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DepPath } from '@pnpm/types'

import type { AuditIndexOptions, AuditPathIndex, DependencyEdge } from './auditIndexTypes.js'
import { appendNamedDepPaths, snapshotChildren, type SnapshotChildrenOptions } from './dependencyEdges.js'
import { createReachableVulnerabilitiesGetter, parseVulnerabilityKey } from './reachableVulnerabilities.js'

// Traverse the lockfile graph without the global depPath de-duplication that
// `@pnpm/lockfile.walker` applies. `findings[].paths` is supposed to list every
// distinct install path to a vulnerable package, so a shared transitive
// dependency (e.g. lodash reached via many parents) must contribute one path
// per parent chain, not just the first one the walker encounters. A per-trail
// visited set prevents cycles without suppressing distinct paths.
export interface WalkForPathsCtx {
  lockfile: LockfileObject
  vulnerableNames: Set<string>
  paths: AuditPathIndex
  depTypes: DepTypes
  optionalOnly: Set<DepPath>
  include?: AuditIndexOptions['include']
  resolvePeersFromWorkspaceRoot?: boolean
  importerSegmentOf: (importerId: string) => string
}

interface PathWalkFrame {
  depPath: DepPath
  trail: TrailNode
  children: DependencyEdge[]
  next: number
}

interface PathWalk {
  ctx: WalkForPathsCtx
  packages: PackageSnapshots
  childOpts: SnapshotChildrenOptions
  reachableVulnerabilities: (edge: DependencyEdge) => ReadonlySet<string>
  // Tracks the depPaths on the current DFS trail so cycles terminate. A frame is
  // added when its node is opened and removed when the frame is unwound, so the
  // set always reflects the path from the root to the current node. Explicit
  // stack rather than recursion: a lockfile is untrusted input, and a deep
  // dependency chain would otherwise overflow the call stack and crash the audit.
  inTrail: Set<DepPath>
  // The trail is a parent-linked chain of names rather than a copied array per
  // node: copying would cost O(depth) memory and time at every node and make a
  // deep chain O(depth^2). The chain is materialized into a path string only
  // when a vulnerable node is recorded.
  stack: PathWalkFrame[]
  // Findings the current importer has already contributed a path to. Each
  // importer records its first path to a finding even past the per-finding
  // cap, so a project with a heavily shared dependency cannot hide that another
  // project depends on the same vulnerable package.
  importerFindings: Set<string>
}

export function walkForPaths (ctx: WalkForPathsCtx): void {
  const childOpts: SnapshotChildrenOptions = {
    includeOptDeps: ctx.include?.optionalDependencies !== false,
    skippedPeerEdges: getPeerSatisfactionEdgesToSkip(ctx.lockfile, ctx),
  }
  const walk: PathWalk = {
    ctx,
    packages: ctx.lockfile.packages ?? {},
    childOpts,
    reachableVulnerabilities: createReachableVulnerabilitiesGetter(ctx.lockfile, ctx.vulnerableNames, childOpts),
    inTrail: new Set(),
    stack: [],
    importerFindings: new Set(),
  }

  for (const [importerId, importer] of Object.entries(ctx.lockfile.importers)) {
    const trail: TrailNode = { name: ctx.importerSegmentOf(importerId), parent: null }
    walk.importerFindings = new Set()
    for (const root of collectImporterRoots(importer, ctx.include)) {
      walkFromRoot(walk, root, trail)
    }
  }
}

function collectImporterRoots (importer: ProjectSnapshot, include: AuditIndexOptions['include']): DependencyEdge[] {
  const roots: DependencyEdge[] = []
  if (include?.dependencies !== false) appendNamedDepPaths(roots, importer.dependencies ?? {})
  if (include?.devDependencies !== false) appendNamedDepPaths(roots, importer.devDependencies ?? {})
  if (include?.optionalDependencies !== false) appendNamedDepPaths(roots, importer.optionalDependencies ?? {})
  return roots
}

function walkFromRoot (walk: PathWalk, root: DependencyEdge, importerTrail: TrailNode): void {
  openNode(walk, root, importerTrail)
  while (walk.stack.length > 0) {
    const frame = walk.stack[walk.stack.length - 1]
    if (frame.next < frame.children.length) {
      openNode(walk, frame.children[frame.next++], frame.trail)
    } else {
      walk.inTrail.delete(frame.depPath)
      walk.stack.pop()
    }
  }
}

// Apply the per-node logic and, unless the node is pruned, push a frame so its
// children are visited. Records a path when the node is itself vulnerable.
function openNode (walk: PathWalk, edge: DependencyEdge, parentTrail: TrailNode): void {
  const { ctx } = walk
  const reachable = walk.reachableVulnerabilities(edge)
  if (reachable.size === 0 || areReachableVulnerabilitiesSaturated(walk, reachable)) return
  if (walk.inTrail.has(edge.depPath)) return
  const pkgSnapshot = walk.packages[edge.depPath]
  if (pkgSnapshot == null) return
  const { name, version } = nameVerFromPkgSnapshot(edge.depPath, pkgSnapshot)
  const resolvedName = name ?? edge.name
  const trail: TrailNode = { name: resolvedName, parent: parentTrail }
  if (version && ctx.vulnerableNames.has(resolvedName)) {
    const findingKey = `${resolvedName}\0${version}`
    recordPath(ctx.paths, resolvedName, version, joinTrail(trail), {
      isDev: ctx.depTypes[edge.depPath] === DepType.DevOnly,
      isOptional: ctx.optionalOnly.has(edge.depPath),
      exceedCap: !walk.importerFindings.has(findingKey),
    })
    walk.importerFindings.add(findingKey)
  }
  if (areReachableVulnerabilitiesSaturated(walk, reachable)) return
  const children = snapshotChildren({ depPath: edge.depPath, snapshot: pkgSnapshot }, walk.childOpts)
  walk.inTrail.add(edge.depPath)
  walk.stack.push({ depPath: edge.depPath, trail, children, next: 0 })
}

// A node in the current DFS trail. Linking to the parent rather than copying the
// whole path keeps per-node memory O(1); `joinTrail` walks the chain to the root
// to produce the `a>b>c` string only when a path is actually recorded.
interface TrailNode {
  name: string
  parent: TrailNode | null
}

function joinTrail (node: TrailNode): string {
  const parts: string[] = []
  let current: TrailNode | null = node
  while (current != null) {
    parts.push(current.name)
    current = current.parent
  }
  parts.reverse()
  return parts.join('>')
}

function areReachableVulnerabilitiesSaturated (walk: PathWalk, reachable: ReadonlySet<string>): boolean {
  for (const key of reachable) {
    if (!isVulnerabilitySaturated(walk, key)) return false
  }
  return true
}

function isVulnerabilitySaturated (walk: PathWalk, vulnerabilityKey: string): boolean {
  const { name, version, depPath } = parseVulnerabilityKey(vulnerabilityKey)
  const info = walk.ctx.paths[name]?.get(version)
  if (!info || info.paths.length < MAX_PATHS_PER_FINDING) return false
  if (!walk.importerFindings.has(`${name}\0${version}`)) return false
  if (walk.ctx.depTypes[depPath] !== DepType.DevOnly && info.dev) return false
  return walk.ctx.optionalOnly.has(depPath) || !info.optional
}

// Per-(name, version) cap on recorded paths. The CLI only ever displays the
// first few and follows with a "run pnpm why" hint, so keeping tens of
// thousands of equivalent chains is wasted memory/CPU for projects with
// heavy sharing (e.g. diamond dependencies deep in the graph). A path with
// `exceedCap` set (an importer's first path to the finding) is recorded
// regardless, so the total is bounded by the cap plus the number of importers.
const MAX_PATHS_PER_FINDING = 100

interface RecordPathOptions {
  isDev: boolean
  isOptional: boolean
  exceedCap: boolean
}

function recordPath (paths: AuditPathIndex, name: string, version: string, joined: string, { isDev, isOptional, exceedCap }: RecordPathOptions): void {
  let byVersion = paths[name]
  if (!byVersion) {
    byVersion = new Map()
    paths[name] = byVersion
  }
  const info = byVersion.get(version)
  if (!info) {
    byVersion.set(version, { paths: [joined], dev: isDev, optional: isOptional })
    return
  }
  if (!isDev) info.dev = false
  if (!isOptional) info.optional = false
  if (info.paths.length >= MAX_PATHS_PER_FINDING && !exceedCap) return
  // Dedupe — the same joined trail can be produced when a package appears in
  // both `dependencies` and `optionalDependencies` of the same parent, or via
  // equivalent peer-suffix variants.
  if (info.paths.includes(joined)) return
  info.paths.push(joined)
}
