import {
  checkPackageInstallability,
  type InstallabilityManifest,
  packageIsInstallable,
} from '@pnpm/config.package-is-installable'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import * as dp from '@pnpm/deps.path'
import { LockfileMissingDependencyError } from '@pnpm/error'
import type {
  LockfileObject,
  PackageSnapshots,
} from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import type { DependenciesField, DepPath, ProjectId, SupportedArchitectures } from '@pnpm/types'
import { map as mapValues, pickBy, unnest } from 'ramda'

import { filterImporter } from './filterImporter.js'

const lockfileLogger = logger('lockfile')

export interface FilterLockfileResult {
  lockfile: LockfileObject
  selectedImporterIds: ProjectId[]
  /**
   * Dep paths reached by a non-optional edge from an importer or from a
   * package that is itself part of the install. Their installability is
   * evaluated as non-optional — an incompatible one fails the install under
   * `engineStrict` instead of being skipped — even when the lockfile marks
   * the snapshot `optional: true` because the subtree happens to hang off an
   * optional dependency. Downstream consumers must classify by this set
   * rather than by `pkgSnapshot.optional` so every stage of a headless
   * install agrees with the resolver, which classifies by edge too.
   */
  requiredDepPaths: Set<DepPath>
}

export function filterLockfileByEngine (
  lockfile: LockfileObject,
  opts: FilterLockfileOptions
): FilterLockfileResult {
  const importerIds = Object.keys(lockfile.importers) as ProjectId[]
  return filterLockfileByImportersAndEngine(lockfile, importerIds, opts)
}

export interface FilterLockfileOptions {
  currentEngine: {
    nodeVersion?: string
    pnpmVersion: string
  }
  engineStrict: boolean
  include: { [dependenciesField in DependenciesField]: boolean }
  includeIncompatiblePackages?: boolean
  failOnMissingDependencies: boolean
  lockfileDir: string
  skipped: Set<string>
  skipRuntimes?: boolean
  supportedArchitectures?: SupportedArchitectures
}

export function filterLockfileByImportersAndEngine (
  lockfile: LockfileObject,
  importerIds: ProjectId[],
  opts: FilterLockfileOptions
): FilterLockfileResult {
  const importerIdSet = new Set(importerIds)

  const directDepEdges = toImporterDepPaths(lockfile, importerIds, {
    include: opts.include,
    importerIdSet,
    skipRuntimes: opts.skipRuntimes,
  })

  const { packages, requiredDepPaths } =
    lockfile.packages != null
      ? pickPkgsWithAllDeps(lockfile, directDepEdges, importerIdSet, {
        currentEngine: opts.currentEngine,
        engineStrict: opts.engineStrict,
        failOnMissingDependencies: opts.failOnMissingDependencies,
        include: opts.include,
        includeIncompatiblePackages:
            opts.includeIncompatiblePackages === true,
        lockfileDir: opts.lockfileDir,
        skipped: opts.skipped,
        skipRuntimes: opts.skipRuntimes,
        supportedArchitectures: opts.supportedArchitectures,
      })
      : { packages: {}, requiredDepPaths: new Set<DepPath>() }

  const importers = mapValues((importer) => {
    const newImporter = filterImporter(importer, opts.include, { skipRuntimes: opts.skipRuntimes })
    if (newImporter.optionalDependencies != null) {
      newImporter.optionalDependencies = pickBy((ref, depName) => {
        const depPath = dp.refToRelative(ref, depName)
        return !depPath || packages[depPath] != null
      }, newImporter.optionalDependencies)
    }
    return newImporter
  }, lockfile.importers)

  return {
    lockfile: {
      ...lockfile,
      importers,
      packages,
    },
    selectedImporterIds: Array.from(importerIdSet),
    requiredDepPaths,
  }
}

interface PickPkgsOptions {
  currentEngine: {
    nodeVersion?: string
    pnpmVersion: string
  }
  engineStrict: boolean
  failOnMissingDependencies: boolean
  include: { [dependenciesField in DependenciesField]: boolean }
  includeIncompatiblePackages: boolean
  lockfileDir: string
  skipped: Set<string>
  skipRuntimes?: boolean
  supportedArchitectures?: SupportedArchitectures
}

interface PickPkgsContext {
  lockfile: LockfileObject
  pickedPackages: PackageSnapshots
  importerIdSet: Set<ProjectId>
  /** Dep paths that are part of the install. */
  installed: Set<DepPath>
  requiredDepPaths: Set<DepPath>
  /**
   * Dep paths an edge out of the install reaches, in the order the walk
   * reached them. These are the ones whose installability is reported;
   * anything below a package that is not itself installed is skipped without
   * being checked.
   */
  evaluated: DepPath[]
  /**
   * Outbound edges of the packages the classification pass expanded, so the
   * closure pass looks them up instead of re-parsing every dependency ref.
   */
  edgesByDepPath: Map<DepPath, DepEdge[]>
}

function pickPkgsWithAllDeps (
  lockfile: LockfileObject,
  depEdges: DepEdge[],
  importerIdSet: Set<ProjectId>,
  opts: PickPkgsOptions
): { packages: PackageSnapshots, requiredDepPaths: Set<DepPath> } {
  const ctx: PickPkgsContext = {
    lockfile,
    pickedPackages: {} as PackageSnapshots,
    importerIdSet,
    installed: new Set(),
    requiredDepPaths: new Set(),
    evaluated: [],
    edgesByDepPath: new Map(),
  }
  classifyDeps(ctx, depEdges, opts)
  reportInstallability(ctx, opts)
  pickSkippedDeps(ctx, depEdges, opts)
  return { packages: ctx.pickedPackages, requiredDepPaths: ctx.requiredDepPaths }
}

/**
 * Work out which packages the install actually reaches, and over which kind
 * of edge.
 *
 * An edge is only expanded once its source is known to be part of the
 * install, so the optionality of the edge — not the `optional` flag the
 * resolver propagated onto the snapshot — decides whether an incompatible
 * package is skipped: only a package whose every inbound edge is optional
 * stays out of the install.
 *
 * Traversal is breadth-first over edges rather than depth-first over
 * packages so the verdict does not depend on which edge happens to reach a
 * package first: a package left out on an optional edge is reconsidered when
 * a non-optional one arrives later. Nothing is reported from here — a
 * package can be visited once per inbound edge, and its installability must
 * be reported exactly once, which [reportInstallability] does afterwards.
 */
function classifyDeps (ctx: PickPkgsContext, depEdges: DepEdge[], opts: PickPkgsOptions): void {
  const incompatible = new Map<DepPath, boolean>()
  // Walked with a cursor rather than `shift()`, which is O(n) per dequeue on
  // a JS array and would make the walk quadratic on a large lockfile.
  const queue = [...depEdges]
  for (let next = 0; next < queue.length; next++) {
    const { depPath, optional } = queue[next]
    if (!optional) ctx.requiredDepPaths.add(depPath)
    if (ctx.installed.has(depPath)) continue
    const pkgSnapshot = ctx.lockfile.packages![depPath]
    // Missing entries are reported by the closure pass, which reaches every
    // dep path this one does.
    if (!pkgSnapshot) continue
    if (!incompatible.has(depPath)) {
      ctx.evaluated.push(depPath)
      // TODO: depPath is not the package ID. Should be fixed
      incompatible.set(depPath, !opts.includeIncompatiblePackages && checkPackageInstallability(
        pkgSnapshot.id ?? depPath,
        toInstallabilityManifest(depPath, pkgSnapshot),
        {
          nodeVersion: opts.currentEngine.nodeVersion,
          optional: true,
          supportedArchitectures: opts.supportedArchitectures,
        }
      ) != null)
    }
    if (optional && incompatible.get(depPath)) continue
    ctx.installed.add(depPath)
    ctx.pickedPackages[depPath] = pkgSnapshot
    const edges = nextDepEdges(ctx, pkgSnapshot, opts)
    ctx.edgesByDepPath.set(depPath, edges)
    // Appended one by one: `push(...edges)` passes each edge as its own
    // argument and overflows the engine's argument limit on a wide enough
    // dependency list.
    for (const edge of edges) {
      queue.push(edge)
    }
  }
}

/**
 * Report each reached package once, against the edge kind it was classified
 * by. This is where an incompatible package that a non-optional edge reaches
 * fails the install under `engineStrict`, and where a skipped optional gets
 * its `skippedOptionalDependencyLogger` entry.
 */
function reportInstallability (ctx: PickPkgsContext, opts: PickPkgsOptions): void {
  for (const depPath of ctx.evaluated) {
    const pkgSnapshot = ctx.lockfile.packages![depPath]
    const installable =
      opts.includeIncompatiblePackages ||
      packageIsInstallable(
        pkgSnapshot.id ?? depPath,
        toInstallabilityManifest(depPath, pkgSnapshot),
        {
          // A subtree hanging off an `optionalDependencies` entry stays
          // best-effort: the dependency is installed so its dependent is not
          // left broken, but an incompatibility inside it does not fail the
          // install. Only a package no optional path reaches is fatal here.
          engineStrict: opts.engineStrict && pkgSnapshot.optional !== true,
          lockfileDir: opts.lockfileDir,
          nodeVersion: opts.currentEngine.nodeVersion,
          optional: !ctx.installed.has(depPath) || !ctx.requiredDepPaths.has(depPath),
          supportedArchitectures: opts.supportedArchitectures,
        }
      ) !== false
    if (installable) {
      opts.skipped.delete(depPath)
    } else {
      opts.skipped.add(depPath)
    }
  }
}

function toInstallabilityManifest (depPath: DepPath, pkgSnapshot: PackageSnapshots[DepPath]): InstallabilityManifest {
  return {
    ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
    cpu: pkgSnapshot.cpu,
    engines: pkgSnapshot.engines,
    os: pkgSnapshot.os,
    libc: pkgSnapshot.libc,
  }
}

/**
 * Extend the picked set over the packages that are reachable but not part of
 * the install — an incompatible optional and everything below it. They stay
 * in the filtered lockfile and are recorded as skipped, so a later install on
 * a host that does support them can pick them up without a re-resolution.
 */
function pickSkippedDeps (ctx: PickPkgsContext, depEdges: DepEdge[], opts: PickPkgsOptions): void {
  const queue = depEdges.map(({ depPath }) => depPath)
  const visited = new Set<DepPath>()
  for (let next = 0; next < queue.length; next++) {
    const depPath = queue[next]
    if (visited.has(depPath)) continue
    visited.add(depPath)
    const pkgSnapshot = ctx.lockfile.packages![depPath]
    if (!pkgSnapshot && !depPath.startsWith('link:')) {
      if (opts.failOnMissingDependencies) {
        throw new LockfileMissingDependencyError(depPath)
      }
      lockfileLogger.debug(`No entry for "${depPath}" in ${WANTED_LOCKFILE}`)
      continue
    }
    if (!ctx.installed.has(depPath)) {
      if (!ctx.pickedPackages[depPath] && pkgSnapshot.optional === true) {
        opts.skipped.add(depPath)
      }
      ctx.pickedPackages[depPath] = pkgSnapshot
    }
    // `visited` guarantees one pass per dep path, so a cached entry is
    // released as soon as it is consumed rather than being retained until the
    // whole walk ends.
    const edges = ctx.edgesByDepPath.get(depPath) ?? nextDepEdges(ctx, pkgSnapshot, opts)
    ctx.edgesByDepPath.delete(depPath)
    for (const edge of edges) {
      queue.push(edge.depPath)
    }
  }
}

/** The outbound edges of a package, tagged with the optionality of each. */
function nextDepEdges (ctx: PickPkgsContext, pkgSnapshot: PackageSnapshots[DepPath], opts: PickPkgsOptions): DepEdge[] {
  const { depEdges, importerIds } = parseDepRefs([
    ...toDepRefs(pkgSnapshot.dependencies, false),
    ...(opts.include.optionalDependencies ? toDepRefs(pkgSnapshot.optionalDependencies, true) : []),
  ], ctx.lockfile)
  for (const importerId of importerIds) {
    ctx.importerIdSet.add(importerId)
  }
  return [
    ...depEdges,
    ...toImporterDepPaths(ctx.lockfile, importerIds, {
      include: opts.include,
      importerIdSet: ctx.importerIdSet,
      skipRuntimes: opts.skipRuntimes,
    }),
  ]
}

function toImporterDepPaths (
  lockfile: LockfileObject,
  importerIds: ProjectId[],
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean }
    importerIdSet: Set<ProjectId>
    skipRuntimes?: boolean
  }
): DepEdge[] {
  const importerDeps = importerIds
    .map(importerId => lockfile.importers[importerId])
    .map(importer => [
      ...(opts.include.dependencies ? toDepRefs(importer.dependencies, false) : []),
      ...(opts.include.devDependencies ? toDepRefs(importer.devDependencies, false) : []),
      ...(opts.include.optionalDependencies ? toDepRefs(importer.optionalDependencies, true) : []),
    ])
    .map(refs => opts.skipRuntimes ? refs.filter(({ ref }) => !ref.startsWith('runtime:')) : refs)

  let { depEdges, importerIds: nextImporterIds } = parseDepRefs(unnest(importerDeps), lockfile)

  if (!nextImporterIds.length) {
    return depEdges
  }
  nextImporterIds = nextImporterIds.filter(importerId => !opts.importerIdSet.has(importerId))
  for (const importerId of nextImporterIds) {
    opts.importerIdSet.add(importerId)
  }
  return [
    ...depEdges,
    ...toImporterDepPaths(lockfile, nextImporterIds, opts),
  ]
}

/** A dependency edge, tagged with whether the dependency is declared optional by its dependent. */
interface DepEdge {
  depPath: DepPath
  optional: boolean
}

interface DepRef {
  pkgName: string
  ref: string
  optional: boolean
}

function toDepRefs (refsByPkgNames: Record<string, string> | undefined, optional: boolean): DepRef[] {
  if (refsByPkgNames == null) return []
  return Object.entries(refsByPkgNames).map(([pkgName, ref]) => ({ pkgName, ref, optional }))
}

interface ParsedDepRefs {
  depEdges: DepEdge[]
  importerIds: ProjectId[]
}

function parseDepRefs (depRefs: DepRef[], lockfile: LockfileObject): ParsedDepRefs {
  const acc: ParsedDepRefs = {
    depEdges: [],
    importerIds: [],
  }
  for (const { pkgName, ref, optional } of depRefs) {
    if (ref.startsWith('link:')) {
      const importerId = ref.substring(5) as ProjectId
      if (lockfile.importers[importerId]) {
        acc.importerIds.push(importerId)
      }
      continue
    }
    const depPath = dp.refToRelative(ref, pkgName)
    if (depPath == null) continue
    acc.depEdges.push({ depPath, optional })
  }
  return acc
}
