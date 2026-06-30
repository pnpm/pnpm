import * as dp from '@pnpm/deps.path'
import { DepType, type DepTypes, detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import { convertToLockfileObject } from '@pnpm/lockfile.fs'
import type { EnvLockfile, LockfileObject, ResolvedDependencies } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { lockfileWalkerGroupImporterSteps, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import type { DependenciesField, DepPath, ProjectId } from '@pnpm/types'

export interface PathInfo {
  paths: string[]
  dev: boolean
  optional: boolean
}

// Versions installed per package name, keyed by version.
export type AuditPathIndex = Record<string, Map<string, PathInfo>>

export interface AuditIndexRequest {
  // Flat map suitable as the POST body for `/advisories/bulk`.
  request: Record<string, string[]>
  totalDependencies: number
  // Production dependencies: neither dev-only nor optional-only. Kept as a
  // distinct counter because devOnly and optionalOnly aren't mutually
  // exclusive — a (name, version) can be both — so `total - dev - optional`
  // would double-subtract those entries.
  dependencies: number
  devDependencies: number
  optionalDependencies: number
}

export interface AuditIndexOptions {
  envLockfile?: EnvLockfile | null
  include?: { [dependenciesField in DependenciesField]: boolean }
  // Pre-computed dep types. Callers that also call buildAuditPathIndex on the
  // same lockfile can share this to avoid walking the lockfile twice.
  depTypes?: DepTypes
  // Pre-computed optional-only depPaths for the main lockfile. Shared between
  // lockfileToAuditRequest and buildAuditPathIndex when both are called.
  optionalOnly?: Set<DepPath>
}

export function lockfileToAuditRequest (
  lockfile: LockfileObject,
  opts: AuditIndexOptions
): AuditIndexRequest {
  const importerIds = Object.keys(lockfile.importers) as ProjectId[]
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, importerIds, { include: opts.include })
  const depTypes = opts.depTypes ?? detectDepTypes(lockfile)
  const optionalOnly = opts.optionalOnly ?? collectOptionalOnlyDepPaths(lockfile, opts.include)

  // Use null-prototype objects for records keyed by package names so a
  // hostile or unusual package name (e.g. "__proto__") cannot pollute the
  // prototype or overwrite inherited properties.
  const request: Record<string, string[]> = Object.create(null)
  // Per (name, version) classification. Counted as dev/optional only while
  // every observed occurrence is dev-only / optional-only; once a non-dev or
  // non-optional occurrence is seen, the flag is cleared and the counter
  // decremented.
  const versionStatesByName: Record<string, Map<string, { devOnly: boolean, optionalOnly: boolean }>> = Object.create(null)
  let totalDependencies = 0
  let dependencies = 0
  let devDependencies = 0
  let optionalDependencies = 0

  const registerOccurrence = (o: { name: string, version: string, devOnly: boolean, optionalOnly: boolean }): void => {
    let versionStates = versionStatesByName[o.name]
    if (!versionStates) {
      versionStates = new Map()
      versionStatesByName[o.name] = versionStates
      request[o.name] = []
    }
    const state = versionStates.get(o.version)
    if (!state) {
      versionStates.set(o.version, { devOnly: o.devOnly, optionalOnly: o.optionalOnly })
      request[o.name].push(o.version)
      totalDependencies++
      if (o.devOnly) devDependencies++
      if (o.optionalOnly) optionalDependencies++
      if (!o.devOnly && !o.optionalOnly) dependencies++
      return
    }
    const wasProduction = !state.devOnly && !state.optionalOnly
    if (state.devOnly && !o.devOnly) {
      state.devOnly = false
      devDependencies--
    }
    if (state.optionalOnly && !o.optionalOnly) {
      state.optionalOnly = false
      optionalDependencies--
    }
    if (!wasProduction && !state.devOnly && !state.optionalOnly) {
      dependencies++
    }
  }

  // Build a visitor for one lockfile graph. The walker already de-duplicates
  // by depPath internally, so we don't need a second visited set here. An
  // explicit frame stack stands in for recursion (registering each dependency
  // before descending, preserving pre-order) so a deep dependency chain from an
  // untrusted lockfile cannot overflow the call stack.
  const makeVisitor = (graphDepTypes: DepTypes, graphOptionalOnly: Set<DepPath>) => {
    return (rootStep: LockfileWalkerStep): void => {
      const stack: Array<{ dependencies: LockfileWalkerStep['dependencies'], next: number }> = [{ dependencies: rootStep.dependencies, next: 0 }]
      while (stack.length > 0) {
        const frame = stack[stack.length - 1]
        if (frame.next >= frame.dependencies.length) {
          stack.pop()
          continue
        }
        const { depPath, pkgSnapshot, next } = frame.dependencies[frame.next++]
        const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        if (version) {
          registerOccurrence({
            name,
            version,
            devOnly: graphDepTypes[depPath] === DepType.DevOnly,
            optionalOnly: graphOptionalOnly.has(depPath),
          })
        }
        stack.push({ dependencies: next().dependencies, next: 0 })
      }
    }
  }

  const visitMain = makeVisitor(depTypes, optionalOnly)
  for (const importerWalker of importerWalkers) {
    visitMain(importerWalker.step)
  }
  if (opts.envLockfile) {
    const envLockfileObject = envLockfileToLockfileObject(opts.envLockfile)
    const envDepTypes = detectDepTypes(envLockfileObject)
    const envOptionalOnly = collectOptionalOnlyDepPaths(envLockfileObject, opts.include)
    const visitEnv = makeVisitor(envDepTypes, envOptionalOnly)
    for (const { step } of lockfileWalkerGroupImporterSteps(envLockfileObject, Object.keys(envLockfileObject.importers) as ProjectId[], { include: opts.include })) {
      visitEnv(step)
    }
  }

  return { request, totalDependencies, dependencies, devDependencies, optionalDependencies }
}

export function buildAuditPathIndex (
  lockfile: LockfileObject,
  vulnerableNames: Set<string>,
  opts: AuditIndexOptions
): AuditPathIndex {
  // Null-prototype record keyed by package name to avoid prototype pollution
  // from registry-supplied or lockfile-supplied names.
  const paths: AuditPathIndex = Object.create(null)
  const depTypes = opts.depTypes ?? detectDepTypes(lockfile)
  const optionalOnly = opts.optionalOnly ?? collectOptionalOnlyDepPaths(lockfile, opts.include)

  walkForPaths({
    lockfile,
    vulnerableNames,
    paths,
    depTypes,
    optionalOnly,
    include: opts.include,
    importerSegmentOf: (importerId) => importerId.replace(/\//g, '__'),
  })

  if (opts.envLockfile) {
    const envLockfileObject = envLockfileToLockfileObject(opts.envLockfile)
    walkForPaths({
      lockfile: envLockfileObject,
      vulnerableNames,
      paths,
      depTypes: detectDepTypes(envLockfileObject),
      optionalOnly: collectOptionalOnlyDepPaths(envLockfileObject, opts.include),
      include: opts.include,
      importerSegmentOf: (importerId) => importerId,
    })
  }

  return paths
}

// Traverse the lockfile graph without the global depPath de-duplication that
// `@pnpm/lockfile.walker` applies. `findings[].paths` is supposed to list every
// distinct install path to a vulnerable package, so a shared transitive
// dependency (e.g. lodash reached via many parents) must contribute one path
// per parent chain, not just the first one the walker encounters. A per-trail
// visited set prevents cycles without suppressing distinct paths.
interface WalkForPathsCtx {
  lockfile: LockfileObject
  vulnerableNames: Set<string>
  paths: AuditPathIndex
  depTypes: DepTypes
  optionalOnly: Set<DepPath>
  include?: AuditIndexOptions['include']
  importerSegmentOf: (importerId: string) => string
}

function walkForPaths (ctx: WalkForPathsCtx): void {
  const { lockfile, vulnerableNames, paths, depTypes, optionalOnly, include, importerSegmentOf } = ctx
  const includeDeps = include?.dependencies !== false
  const includeDevDeps = include?.devDependencies !== false
  const includeOptDeps = include?.optionalDependencies !== false
  const packages = lockfile.packages ?? {}
  const reachableVulnerabilities = createReachableVulnerabilitiesGetter(lockfile, vulnerableNames, includeOptDeps)

  // Tracks the depPaths on the current DFS trail so cycles terminate. A frame is
  // added when its node is opened and removed when the frame is unwound, so the
  // set always reflects the path from the root to the current node. Explicit
  // stack rather than recursion: a lockfile is untrusted input, and a deep
  // dependency chain would otherwise overflow the call stack and crash the audit.
  const inTrail = new Set<DepPath>()
  // The trail is a parent-linked chain of names rather than a copied array per
  // node: copying would cost O(depth) memory and time at every node and make a
  // deep chain O(depth^2). The chain is materialized into a path string only
  // when a vulnerable node is recorded.
  const stack: Array<{ depPath: DepPath, trail: TrailNode, children: Array<{ name: string, depPath: DepPath }>, next: number }> = []

  // Apply the per-node logic and, unless the node is pruned, push a frame so its
  // children are visited. Records a path when the node is itself vulnerable.
  const open = (edge: { name: string, depPath: DepPath }, parentTrail: TrailNode): void => {
    const reachable = reachableVulnerabilities(edge)
    if (reachable.size === 0 || allReachableVulnerabilitiesSaturated(paths, reachable, depTypes, optionalOnly)) return
    if (inTrail.has(edge.depPath)) return
    const pkgSnapshot = packages[edge.depPath]
    if (pkgSnapshot == null) return
    const { name, version } = nameVerFromPkgSnapshot(edge.depPath, pkgSnapshot)
    const resolvedName = name ?? edge.name
    const trail: TrailNode = { name: resolvedName, parent: parentTrail }
    if (version && vulnerableNames.has(resolvedName)) {
      recordPath(paths, resolvedName, version, joinTrail(trail),
        depTypes[edge.depPath] === DepType.DevOnly,
        optionalOnly.has(edge.depPath))
    }
    if (allReachableVulnerabilitiesSaturated(paths, reachable, depTypes, optionalOnly)) return
    const children: Array<{ name: string, depPath: DepPath }> = []
    appendNamedDepPaths(children, pkgSnapshot.dependencies ?? {})
    if (includeOptDeps) {
      appendNamedDepPaths(children, pkgSnapshot.optionalDependencies ?? {})
    }
    inTrail.add(edge.depPath)
    stack.push({ depPath: edge.depPath, trail, children, next: 0 })
  }

  for (const [importerId, importer] of Object.entries(lockfile.importers)) {
    const trail: TrailNode = { name: importerSegmentOf(importerId), parent: null }
    const roots: Array<{ name: string, depPath: DepPath }> = []
    if (includeDeps) appendNamedDepPaths(roots, importer.dependencies ?? {})
    if (includeDevDeps) appendNamedDepPaths(roots, importer.devDependencies ?? {})
    if (includeOptDeps) appendNamedDepPaths(roots, importer.optionalDependencies ?? {})
    for (const root of roots) {
      open(root, trail)
      while (stack.length > 0) {
        const frame = stack[stack.length - 1]
        if (frame.next < frame.children.length) {
          open(frame.children[frame.next++], frame.trail)
        } else {
          inTrail.delete(frame.depPath)
          stack.pop()
        }
      }
    }
  }
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

// For each node, the set of vulnerabilities reachable from it (itself included),
// used by the walker to prune subtrees that reach no unsaturated finding.
// Tarjan's SCC algorithm scans every node once and shares one set across a
// cycle, avoiding the quadratic recompute of memoizing only acyclic subtrees.
function createReachableVulnerabilitiesGetter (
  lockfile: LockfileObject,
  vulnerableNames: Set<string>,
  includeOptDeps: boolean
): (edge: { name: string, depPath: DepPath }) => ReadonlySet<string> {
  const packages = lockfile.packages ?? {}
  // Final reachable set per node, shared across its SCC.
  const memo = new Map<DepPath, Set<string>>()
  // Per-node contribution, merged into the shared set when the SCC closes.
  const partial = new Map<DepPath, Set<string>>()
  const index = new Map<DepPath, number>()
  const lowlink = new Map<DepPath, number>()
  const onStack = new Set<DepPath>()
  const sccStack: DepPath[] = []
  let counter = 0

  // Iterative Tarjan: an explicit frame stack stands in for the recursive
  // strongconnect, so a deep dependency chain from an untrusted lockfile cannot
  // overflow the call stack. Each frame's `own` set is shared by reference with
  // `partial`, so contributions merged in while the children run are visible
  // when the SCC closes.
  const buildScc = (rootEdge: { name: string, depPath: DepPath }): void => {
    const work: Array<{ edge: { name: string, depPath: DepPath }, own: Set<string>, children: Array<{ name: string, depPath: DepPath }>, next: number }> = []

    const pushFrame = (edge: { name: string, depPath: DepPath }): void => {
      index.set(edge.depPath, counter)
      lowlink.set(edge.depPath, counter)
      counter++
      sccStack.push(edge.depPath)
      onStack.add(edge.depPath)

      // Derive children from this single read rather than via a helper that would
      // read the snapshot again.
      const pkgSnapshot = packages[edge.depPath]
      const own = new Set<string>()
      const children: Array<{ name: string, depPath: DepPath }> = []
      if (pkgSnapshot != null) {
        const { name, version } = nameVerFromPkgSnapshot(edge.depPath, pkgSnapshot)
        const resolvedName = name ?? edge.name
        if (version && vulnerableNames.has(resolvedName)) {
          own.add(vulnerabilityKey(resolvedName, version, edge.depPath))
        }
        appendNamedDepPaths(children, pkgSnapshot.dependencies ?? {})
        if (includeOptDeps) {
          appendNamedDepPaths(children, pkgSnapshot.optionalDependencies ?? {})
        }
      }
      partial.set(edge.depPath, own)
      work.push({ edge, own, children, next: 0 })
    }

    pushFrame(rootEdge)
    while (work.length > 0) {
      const frame = work[work.length - 1]
      if (frame.next < frame.children.length) {
        const child = frame.children[frame.next++]
        if (!index.has(child.depPath)) {
          pushFrame(child)
          continue
        }
        if (onStack.has(child.depPath)) {
          lowlink.set(frame.edge.depPath, Math.min(lowlink.get(frame.edge.depPath)!, index.get(child.depPath)!))
        }
        // Finalized successors are already in `memo`; same-SCC ones are folded in
        // when the SCC closes.
        const childReachable = memo.get(child.depPath)
        if (childReachable) addAll(frame.own, childReachable)
        continue
      }

      const edge = frame.edge
      if (lowlink.get(edge.depPath) === index.get(edge.depPath)) {
        const members: DepPath[] = []
        // Reuse the first member's own set as the shared accumulator instead of
        // allocating a fresh one, so the common singleton-SCC case finalizes
        // without any extra Set allocation or copy.
        let shared: Set<string> | undefined
        let member: DepPath
        do {
          member = sccStack.pop()!
          onStack.delete(member)
          members.push(member)
          const own = partial.get(member)!
          partial.delete(member)
          if (shared === undefined) {
            shared = own
          } else {
            addAll(shared, own)
          }
        } while (member !== edge.depPath)
        for (const m of members) {
          memo.set(m, shared!)
        }
      }

      work.pop()
      // Apply the post-DFS update to the parent: propagate this node's lowlink
      // and fold in its reachable set once finalized (same-SCC nodes are folded
      // later, via `partial`, when the shared SCC root closes).
      const parent = work[work.length - 1]
      if (parent != null) {
        lowlink.set(parent.edge.depPath, Math.min(lowlink.get(parent.edge.depPath)!, lowlink.get(edge.depPath)!))
        const childReachable = memo.get(edge.depPath)
        if (childReachable) addAll(parent.own, childReachable)
      }
    }
  }

  return (edge) => {
    if (!index.has(edge.depPath)) buildScc(edge)
    // strongconnect always finalizes the queried node's SCC, so its reachable
    // set is present afterwards. A missing entry would be a bug that silently
    // under-reports and hides a real finding, so fail loudly instead of
    // returning an empty set.
    const reachable = memo.get(edge.depPath)
    if (reachable == null) {
      throw new Error(`Reachable vulnerabilities were not computed for ${edge.depPath}`)
    }
    return reachable
  }
}

function allReachableVulnerabilitiesSaturated (
  paths: AuditPathIndex,
  reachable: ReadonlySet<string>,
  depTypes: DepTypes,
  optionalOnly: Set<DepPath>
): boolean {
  for (const key of reachable) {
    const { name, version, depPath } = parseVulnerabilityKey(key)
    const info = paths[name]?.get(version)
    if (!info || info.paths.length < MAX_PATHS_PER_FINDING) return false
    if (depTypes[depPath] !== DepType.DevOnly && info.dev) return false
    if (!optionalOnly.has(depPath) && info.optional) return false
  }
  return true
}

function vulnerabilityKey (name: string, version: string, depPath: DepPath): string {
  return `${name}\0${version}\0${depPath}`
}

function parseVulnerabilityKey (key: string): { name: string, version: string, depPath: DepPath } {
  const [name, version, depPath] = key.split('\0')
  return { name, version, depPath: depPath as DepPath }
}

function addAll<T> (target: Set<T>, source: Set<T>): void {
  for (const value of source) {
    target.add(value)
  }
}

// Per-(name, version) cap on recorded paths. The CLI only ever displays the
// first few and follows with a "run pnpm why" hint, so keeping tens of
// thousands of equivalent chains is wasted memory/CPU for projects with
// heavy sharing (e.g. diamond dependencies deep in the graph).
const MAX_PATHS_PER_FINDING = 100

function recordPath (paths: AuditPathIndex, name: string, version: string, joined: string, isDev: boolean, isOptional: boolean): void {
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
  if (info.paths.length >= MAX_PATHS_PER_FINDING) return
  // Dedupe — the same joined trail can be produced when a package appears in
  // both `dependencies` and `optionalDependencies` of the same parent, or via
  // equivalent peer-suffix variants.
  if (info.paths.includes(joined)) return
  info.paths.push(joined)
}

// Append rather than `target.push(...mapped(deps))`: a lockfile is untrusted
// input, and spreading a pathologically large dependency list into push()
// arguments can exceed the engine's argument limit and throw, crashing the
// audit. Appending in a loop also avoids the intermediate array.
function appendNamedDepPaths (target: Array<{ name: string, depPath: DepPath }>, deps: ResolvedDependencies): void {
  for (const [alias, ref] of Object.entries(deps)) {
    const depPath = dp.refToRelative(ref, alias)
    if (depPath != null) target.push({ name: alias, depPath })
  }
}

// Returns the set of depPaths that are reachable only through optional edges
// (i.e. they would be absent from the install set if optionalDependencies were
// not included). Matches the AuditMetadata.optionalDependencies semantic.
//
// Implemented as (reachableWithOptional − reachableWithoutOptional) so that
// optionalDependencies nested inside a required chain are also accounted for,
// not just the ones declared directly on importer.optionalDependencies.
//
// Root selection honours the caller's `include` flags, so running
// `pnpm audit --prod` doesn't let dev-only subgraphs flip a package out of
// "optional-only" classification.
export function collectOptionalOnlyDepPaths (
  lockfile: LockfileObject,
  include?: AuditIndexOptions['include']
): Set<DepPath> {
  const includeDeps = include?.dependencies !== false
  const includeDevDeps = include?.devDependencies !== false
  const includeOptDeps = include?.optionalDependencies !== false
  const withoutOptional = new Set<DepPath>()
  const withOptional = new Set<DepPath>()
  for (const importer of Object.values(lockfile.importers)) {
    const nonOptionalRoots = [
      ...(includeDeps ? resolvedDepsToDepPaths(importer.dependencies ?? {}) : []),
      ...(includeDevDeps ? resolvedDepsToDepPaths(importer.devDependencies ?? {}) : []),
    ]
    const allRoots = [
      ...nonOptionalRoots,
      ...(includeOptDeps ? resolvedDepsToDepPaths(importer.optionalDependencies ?? {}) : []),
    ]
    walkReachable(lockfile, nonOptionalRoots, withoutOptional, false)
    walkReachable(lockfile, allRoots, withOptional, includeOptDeps)
  }
  const result = new Set<DepPath>()
  for (const depPath of withOptional) {
    if (!withoutOptional.has(depPath)) result.add(depPath)
  }
  return result
}

// Explicit stack rather than recursion: a lockfile is untrusted input, and a
// deep dependency chain would otherwise overflow the call stack. Order does not
// matter — the result is the reachable set, so a LIFO walk is equivalent.
function walkReachable (lockfile: LockfileObject, depPaths: DepPath[], seen: Set<DepPath>, includeOptionalEdges: boolean): void {
  const packages = lockfile.packages ?? {}
  const stack: DepPath[] = []
  for (const depPath of depPaths) stack.push(depPath)
  while (stack.length > 0) {
    const depPath = stack.pop()!
    if (seen.has(depPath)) continue
    seen.add(depPath)
    const snapshot = packages[depPath]
    if (!snapshot) continue
    for (const child of resolvedDepsToDepPaths(snapshot.dependencies ?? {})) stack.push(child)
    if (includeOptionalEdges) {
      for (const child of resolvedDepsToDepPaths(snapshot.optionalDependencies ?? {})) stack.push(child)
    }
  }
}

function resolvedDepsToDepPaths (deps: ResolvedDependencies): DepPath[] {
  return Object.entries(deps)
    .map(([alias, ref]) => dp.refToRelative(ref, alias))
    .filter((depPath): depPath is DepPath => depPath !== null)
}

function envLockfileToLockfileObject (envLockfile: EnvLockfile): LockfileObject {
  const envImporter = envLockfile.importers['.']
  const importers: Record<string, { dependencies?: Record<string, { specifier: string, version: string }> }> = {}
  if (Object.keys(envImporter.configDependencies).length > 0) {
    importers['configDependencies'] = { dependencies: envImporter.configDependencies }
  }
  if (envImporter.packageManagerDependencies) {
    importers['packageManagerDependencies'] = { dependencies: envImporter.packageManagerDependencies }
  }
  return convertToLockfileObject({
    lockfileVersion: envLockfile.lockfileVersion,
    importers,
    packages: envLockfile.packages,
    snapshots: envLockfile.snapshots,
  })
}
