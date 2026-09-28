import * as dp from '@pnpm/deps.path'
import {
  getPeerSatisfactionEdgesToSkip,
  isPeerSatisfactionEdge,
  type PeerSatisfactionEdges,
} from '@pnpm/lockfile.peer-edges'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import type { DependenciesField, DepPath, ProjectId } from '@pnpm/types'

export interface LockedDependency {
  depPath: DepPath
  pkgSnapshot: PackageSnapshot
  next: () => LockfileWalkerStep
}

export interface LockfileWalkerStep {
  dependencies: LockedDependency[]
  links: string[]
  missing: string[]
}

export interface LockfileWalkerOptions {
  include?: { [dependenciesField in DependenciesField]: boolean }
  skipped?: Set<DepPath>
  resolvePeersFromWorkspaceRoot?: boolean
  /**
   * The peer-satisfaction edges to skip. Defaults to the ones computed from
   * `lockfile` under `include`. Pass the edges of the unfiltered lockfile when
   * walking a copy whose importers were already filtered.
   */
  peerSatisfactionEdges?: PeerSatisfactionEdges
}

export function lockfileWalkerGroupImporterSteps (
  lockfile: LockfileObject,
  importerIds: ProjectId[],
  opts?: LockfileWalkerOptions
): Array<{ importerId: string, step: LockfileWalkerStep }> {
  const ctx = createWalkerContext(lockfile, opts)

  return importerIds.map((importerId) => {
    const projectSnapshot = lockfile.importers[importerId]
    const entryNodes = Object.entries({
      ...(opts?.include?.devDependencies === false ? {} : projectSnapshot.devDependencies),
      ...(opts?.include?.dependencies === false ? {} : projectSnapshot.dependencies),
      ...(opts?.include?.dependencies === false || opts?.include?.optionalDependencies === false ? {} : projectSnapshot.optionalDependencies),
    })
      .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
      .filter((nodeId) => nodeId !== null) as DepPath[]
    return {
      importerId,
      step: step(ctx, entryNodes),
    }
  })
}

export interface LockfileWalker {
  directDeps: Array<{
    alias: string
    depPath: DepPath
  }>
  step: LockfileWalkerStep
}

export function lockfileWalker (
  lockfile: LockfileObject,
  importerIds: ProjectId[],
  opts?: LockfileWalkerOptions
): LockfileWalker {
  const entryNodes = [] as DepPath[]
  const directDeps = [] as Array<{ alias: string, depPath: DepPath }>

  for (const importerId of importerIds) {
    const projectSnapshot = lockfile.importers[importerId]
    Object.entries({
      ...(opts?.include?.devDependencies === false ? {} : projectSnapshot.devDependencies),
      ...(opts?.include?.dependencies === false ? {} : projectSnapshot.dependencies),
      ...(opts?.include?.dependencies === false || opts?.include?.optionalDependencies === false ? {} : projectSnapshot.optionalDependencies),
    })
      .forEach(([pkgName, reference]) => {
        const depPath = dp.refToRelative(reference, pkgName)
        if (depPath === null) return
        entryNodes.push(depPath)
        directDeps.push({ alias: pkgName, depPath })
      })
  }
  return {
    directDeps,
    step: step(createWalkerContext(lockfile, opts), entryNodes),
  }
}

interface WalkerContext {
  includeOptionalDependencies: boolean
  lockfile: LockfileObject
  walked: Set<DepPath>
  peerSatisfactionEdges?: PeerSatisfactionEdges
}

function createWalkerContext (lockfile: LockfileObject, opts: LockfileWalkerOptions | undefined): WalkerContext {
  return {
    includeOptionalDependencies: opts?.include?.optionalDependencies !== false,
    lockfile,
    walked: new Set<DepPath>(((opts?.skipped) != null) ? Array.from(opts?.skipped) : []),
    peerSatisfactionEdges: opts?.peerSatisfactionEdges ?? getPeerSatisfactionEdgesToSkip(lockfile, {
      include: opts?.include,
      resolvePeersFromWorkspaceRoot: opts?.resolvePeersFromWorkspaceRoot,
    }),
  }
}

function step (
  ctx: WalkerContext,
  nextDepPaths: DepPath[]
): LockfileWalkerStep {
  const result: LockfileWalkerStep = {
    dependencies: [],
    links: [],
    missing: [],
  }
  for (const depPath of nextDepPaths) {
    if (ctx.walked.has(depPath)) continue
    ctx.walked.add(depPath)
    const pkgSnapshot = ctx.lockfile.packages?.[depPath]
    if (pkgSnapshot == null) {
      if (depPath.startsWith('link:')) {
        result.links.push(depPath)
        continue
      }
      result.missing.push(depPath)
      continue
    }
    result.dependencies.push({
      depPath,
      next: () => step(ctx, next(ctx, depPath, pkgSnapshot)),
      pkgSnapshot,
    })
  }
  return result
}

function next (ctx: WalkerContext, depPath: DepPath, nextPkg: PackageSnapshot): DepPath[] {
  return Object.entries({
    ...nextPkg.dependencies,
    ...(ctx.includeOptionalDependencies ? nextPkg.optionalDependencies : {}),
  })
    .filter(([pkgName]) => !isPeerSatisfactionEdge(ctx.peerSatisfactionEdges, depPath, pkgName))
    .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null) as DepPath[]
}
