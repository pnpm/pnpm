import * as dp from '@pnpm/deps.path'
import {
  getPeerSatisfactionEdgesToSkip,
  isPeerSatisfactionEdge,
  type PeerSatisfactionEdges,
} from '@pnpm/lockfile.peer-edges'
import type { LockfileObject, PackageSnapshot, ResolvedDependencies } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'

import type { AuditIndexOptions, DependencyEdge } from './auditIndexTypes.js'

// Append rather than `target.push(...mapped(deps))`: a lockfile is untrusted
// input, and spreading a pathologically large dependency list into push()
// arguments can exceed the engine's argument limit and throw, crashing the
// audit. Appending in a loop also avoids the intermediate array.
export function appendNamedDepPaths (
  target: DependencyEdge[],
  deps: ResolvedDependencies,
  isSkipped?: (alias: string) => boolean
): void {
  for (const [alias, ref] of Object.entries(deps)) {
    if (isSkipped?.(alias)) continue
    const depPath = dp.refToRelative(ref, alias)
    if (depPath != null) target.push({ name: alias, depPath })
  }
}

export interface SnapshotChildrenOptions {
  includeOptDeps: boolean
  skippedPeerEdges: PeerSatisfactionEdges | undefined
}

export function snapshotChildren (
  parent: { depPath: DepPath, snapshot: PackageSnapshot },
  opts: SnapshotChildrenOptions
): DependencyEdge[] {
  const children: DependencyEdge[] = []
  const isSkipped = opts.skippedPeerEdges?.has(parent.depPath)
    ? (alias: string) => isPeerSatisfactionEdge(opts.skippedPeerEdges, parent.depPath, alias)
    : undefined
  appendNamedDepPaths(children, parent.snapshot.dependencies ?? {}, isSkipped)
  if (opts.includeOptDeps) {
    appendNamedDepPaths(children, parent.snapshot.optionalDependencies ?? {}, isSkipped)
  }
  return children
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
  opts: Pick<AuditIndexOptions, 'include' | 'resolvePeersFromWorkspaceRoot'>
): Set<DepPath> {
  const includeDeps = opts.include?.dependencies !== false
  const includeDevDeps = opts.include?.devDependencies !== false
  const includeOptDeps = opts.include?.optionalDependencies !== false
  const withoutOptional = new Set<DepPath>()
  const withOptional = new Set<DepPath>()
  const withOptionalChildOpts: SnapshotChildrenOptions = {
    includeOptDeps,
    skippedPeerEdges: getPeerSatisfactionEdgesToSkip(lockfile, opts),
  }
  const withoutOptionalChildOpts: SnapshotChildrenOptions = {
    includeOptDeps: false,
    skippedPeerEdges: getPeerSatisfactionEdgesToSkip(lockfile, {
      include: { dependencies: includeDeps, devDependencies: includeDevDeps, optionalDependencies: false },
      resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    }),
  }
  for (const importer of Object.values(lockfile.importers)) {
    const nonOptionalRoots = [
      ...(includeDeps ? resolvedDepsToDepPaths(importer.dependencies ?? {}) : []),
      ...(includeDevDeps ? resolvedDepsToDepPaths(importer.devDependencies ?? {}) : []),
    ]
    const allRoots = [
      ...nonOptionalRoots,
      ...(includeOptDeps ? resolvedDepsToDepPaths(importer.optionalDependencies ?? {}) : []),
    ]
    walkReachable(lockfile, nonOptionalRoots, { seen: withoutOptional, childOpts: withoutOptionalChildOpts })
    walkReachable(lockfile, allRoots, { seen: withOptional, childOpts: withOptionalChildOpts })
  }
  return subtractDepPaths(withOptional, withoutOptional)
}

function subtractDepPaths (minuend: Set<DepPath>, subtrahend: Set<DepPath>): Set<DepPath> {
  const difference = new Set<DepPath>()
  for (const depPath of minuend) {
    if (!subtrahend.has(depPath)) difference.add(depPath)
  }
  return difference
}

// Explicit stack rather than recursion: a lockfile is untrusted input, and a
// deep dependency chain would otherwise overflow the call stack. Order does not
// matter — the result is the reachable set, so a LIFO walk is equivalent.
function walkReachable (
  lockfile: LockfileObject,
  depPaths: DepPath[],
  { seen, childOpts }: { seen: Set<DepPath>, childOpts: SnapshotChildrenOptions }
): void {
  const packages = lockfile.packages ?? {}
  const stack: DepPath[] = []
  for (const depPath of depPaths) stack.push(depPath)
  while (stack.length > 0) {
    const depPath = stack.pop()!
    if (seen.has(depPath)) continue
    seen.add(depPath)
    const snapshot = packages[depPath]
    if (!snapshot) continue
    for (const child of snapshotChildren({ depPath, snapshot }, childOpts)) stack.push(child.depPath)
  }
}

function resolvedDepsToDepPaths (deps: ResolvedDependencies): DepPath[] {
  return Object.entries(deps)
    .map(([alias, ref]) => dp.refToRelative(ref, alias))
    .filter((depPath): depPath is DepPath => depPath !== null)
}
