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
  // by depPath internally, so we don't need a second visited set here.
  const makeVisitor = (graphDepTypes: DepTypes, graphOptionalOnly: Set<DepPath>) => {
    const visit = (step: LockfileWalkerStep): void => {
      for (const { depPath, pkgSnapshot, next } of step.dependencies) {
        const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        if (version) {
          registerOccurrence({
            name,
            version,
            devOnly: graphDepTypes[depPath] === DepType.DevOnly,
            optionalOnly: graphOptionalOnly.has(depPath),
          })
        }
        visit(next())
      }
    }
    return visit
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

  // Reused across every root to avoid per-node Set cloning. visit adds the
  // current depPath before recursing and removes it on the way back, so the
  // set always reflects the current trail.
  const inTrail = new Set<DepPath>()
  const visit = (edge: { name: string, depPath: DepPath }, trail: string[]): void => {
    if (inTrail.has(edge.depPath)) return
    const pkgSnapshot = packages[edge.depPath]
    if (pkgSnapshot == null) return
    const { name, version } = nameVerFromPkgSnapshot(edge.depPath, pkgSnapshot)
    const resolvedName = name ?? edge.name
    const fullPath = [...trail, resolvedName]
    if (version && vulnerableNames.has(resolvedName)) {
      recordPath(paths, resolvedName, version, fullPath.join('>'),
        depTypes[edge.depPath] === DepType.DevOnly,
        optionalOnly.has(edge.depPath))
    }
    inTrail.add(edge.depPath)
    try {
      for (const child of resolvedDepsToNamedDepPaths(pkgSnapshot.dependencies ?? {})) {
        visit(child, fullPath)
      }
      if (includeOptDeps) {
        for (const child of resolvedDepsToNamedDepPaths(pkgSnapshot.optionalDependencies ?? {})) {
          visit(child, fullPath)
        }
      }
    } finally {
      inTrail.delete(edge.depPath)
    }
  }

  for (const [importerId, importer] of Object.entries(lockfile.importers)) {
    const trail = [importerSegmentOf(importerId)]
    const roots: Array<{ name: string, depPath: DepPath }> = []
    if (includeDeps) roots.push(...resolvedDepsToNamedDepPaths(importer.dependencies ?? {}))
    if (includeDevDeps) roots.push(...resolvedDepsToNamedDepPaths(importer.devDependencies ?? {}))
    if (includeOptDeps) roots.push(...resolvedDepsToNamedDepPaths(importer.optionalDependencies ?? {}))
    for (const root of roots) {
      visit(root, trail)
    }
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

function resolvedDepsToNamedDepPaths (deps: ResolvedDependencies): Array<{ name: string, depPath: DepPath }> {
  const result: Array<{ name: string, depPath: DepPath }> = []
  for (const [alias, ref] of Object.entries(deps)) {
    const depPath = dp.refToRelative(ref, alias)
    if (depPath != null) result.push({ name: alias, depPath })
  }
  return result
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

function walkReachable (lockfile: LockfileObject, depPaths: DepPath[], seen: Set<DepPath>, includeOptionalEdges: boolean): void {
  const packages = lockfile.packages ?? {}
  for (const depPath of depPaths) {
    if (seen.has(depPath)) continue
    seen.add(depPath)
    const snapshot = packages[depPath]
    if (!snapshot) continue
    walkReachable(lockfile, resolvedDepsToDepPaths(snapshot.dependencies ?? {}), seen, includeOptionalEdges)
    if (includeOptionalEdges) {
      walkReachable(lockfile, resolvedDepsToDepPaths(snapshot.optionalDependencies ?? {}), seen, includeOptionalEdges)
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
