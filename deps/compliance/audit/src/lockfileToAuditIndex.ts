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
  const optionalOnly = opts.optionalOnly ?? collectOptionalOnlyDepPaths(lockfile)

  const request: Record<string, string[]> = {}
  // Per (name, version) classification. Counted as dev/optional only while
  // every observed occurrence is dev-only / optional-only; once a non-dev or
  // non-optional occurrence is seen, the flag is cleared and the counter
  // decremented so the metadata stays accurate when the same (name, version)
  // is reachable via multiple depPaths (peer-suffix variants, or once via the
  // main graph and once via the env graph).
  const versionStatesByName: Record<string, Map<string, { devOnly: boolean, optionalOnly: boolean }>> = {}
  let totalDependencies = 0
  let devDependencies = 0
  let optionalDependencies = 0

  const registerOccurrence = (name: string, version: string, isDevOnly: boolean, isOptionalOnly: boolean): void => {
    let versionStates = versionStatesByName[name]
    if (!versionStates) {
      versionStates = new Map()
      versionStatesByName[name] = versionStates
      request[name] = []
    }
    const state = versionStates.get(version)
    if (!state) {
      versionStates.set(version, { devOnly: isDevOnly, optionalOnly: isOptionalOnly })
      request[name].push(version)
      totalDependencies++
      if (isDevOnly) devDependencies++
      if (isOptionalOnly) optionalDependencies++
      return
    }
    if (state.devOnly && !isDevOnly) {
      state.devOnly = false
      devDependencies--
    }
    if (state.optionalOnly && !isOptionalOnly) {
      state.optionalOnly = false
      optionalDependencies--
    }
  }

  // Skip subtrees rooted at depPaths we've already traversed within this walk.
  // The current occurrence is still registered above, so classification for an
  // existing (name, version) can still be updated on later visits. Maintain
  // separate seen sets per lockfile so a main-graph dev occurrence doesn't
  // mask the same depPath's non-dev occurrence in the env lockfile.
  const visit = (
    step: LockfileWalkerStep,
    currentDepTypes: DepTypes,
    currentOptionalOnly: Set<DepPath>,
    seenDepPaths: Set<string>
  ): void => {
    for (const { depPath, pkgSnapshot, next } of step.dependencies) {
      const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      if (version) {
        registerOccurrence(
          name,
          version,
          currentDepTypes[depPath] === DepType.DevOnly,
          currentOptionalOnly.has(depPath)
        )
      }
      if (seenDepPaths.has(depPath)) continue
      seenDepPaths.add(depPath)
      visit(next(), currentDepTypes, currentOptionalOnly, seenDepPaths)
    }
  }

  const seenMainDepPaths = new Set<string>()
  for (const importerWalker of importerWalkers) {
    visit(importerWalker.step, depTypes, optionalOnly, seenMainDepPaths)
  }
  if (opts.envLockfile) {
    const envLockfileObject = envLockfileToLockfileObject(opts.envLockfile)
    const envDepTypes = detectDepTypes(envLockfileObject)
    const envOptionalOnly = collectOptionalOnlyDepPaths(envLockfileObject)
    const seenEnvDepPaths = new Set<string>()
    for (const { step } of lockfileWalkerGroupImporterSteps(envLockfileObject, Object.keys(envLockfileObject.importers) as ProjectId[], { include: opts.include })) {
      visit(step, envDepTypes, envOptionalOnly, seenEnvDepPaths)
    }
  }

  return { request, totalDependencies, devDependencies, optionalDependencies }
}

// Returns the set of depPaths that are reachable only through optional edges
// (i.e. they would be absent from the install set if optionalDependencies were
// not included). Matches the AuditMetadata.optionalDependencies semantic.
export function collectOptionalOnlyDepPaths (lockfile: LockfileObject): Set<DepPath> {
  const nonOptional = new Set<DepPath>()
  const optional = new Set<DepPath>()
  for (const importer of Object.values(lockfile.importers)) {
    walkNonOptional(lockfile, resolvedDepsToDepPaths(importer.dependencies ?? {}), nonOptional)
    walkNonOptional(lockfile, resolvedDepsToDepPaths(importer.devDependencies ?? {}), nonOptional)
    walkOptional(lockfile, resolvedDepsToDepPaths(importer.optionalDependencies ?? {}), optional)
  }
  const result = new Set<DepPath>()
  for (const depPath of optional) {
    if (!nonOptional.has(depPath)) result.add(depPath)
  }
  return result
}

function walkNonOptional (lockfile: LockfileObject, depPaths: DepPath[], seen: Set<DepPath>): void {
  const packages = lockfile.packages ?? {}
  for (const depPath of depPaths) {
    if (seen.has(depPath)) continue
    seen.add(depPath)
    const snapshot = packages[depPath]
    if (!snapshot) continue
    walkNonOptional(lockfile, resolvedDepsToDepPaths(snapshot.dependencies ?? {}), seen)
  }
}

function walkOptional (lockfile: LockfileObject, depPaths: DepPath[], seen: Set<DepPath>): void {
  const packages = lockfile.packages ?? {}
  for (const depPath of depPaths) {
    if (seen.has(depPath)) continue
    seen.add(depPath)
    const snapshot = packages[depPath]
    if (!snapshot) continue
    walkOptional(lockfile, resolvedDepsToDepPaths(snapshot.dependencies ?? {}), seen)
    walkOptional(lockfile, resolvedDepsToDepPaths(snapshot.optionalDependencies ?? {}), seen)
  }
}

function resolvedDepsToDepPaths (deps: ResolvedDependencies): DepPath[] {
  return Object.entries(deps)
    .map(([alias, ref]) => dp.refToRelative(ref, alias))
    .filter((depPath): depPath is DepPath => depPath !== null)
}

export function buildAuditPathIndex (
  lockfile: LockfileObject,
  vulnerableNames: Set<string>,
  opts: AuditIndexOptions
): AuditPathIndex {
  const paths: AuditPathIndex = {}
  const importerIds = Object.keys(lockfile.importers) as ProjectId[]
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, importerIds, { include: opts.include })
  const depTypes = opts.depTypes ?? detectDepTypes(lockfile)
  const optionalOnly = opts.optionalOnly ?? collectOptionalOnlyDepPaths(lockfile)

  const walk = (step: LockfileWalkerStep, currentDepTypes: DepTypes, currentOptionalOnly: Set<DepPath>, trail: string[]): void => {
    for (const { depPath, pkgSnapshot, next } of step.dependencies) {
      const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      const fullPath = [...trail, name]
      if (version && vulnerableNames.has(name)) {
        const isDev = currentDepTypes[depPath] === DepType.DevOnly
        const isOptional = currentOptionalOnly.has(depPath)
        let byVersion = paths[name]
        if (!byVersion) {
          byVersion = new Map()
          paths[name] = byVersion
        }
        const info = byVersion.get(version)
        if (!info) {
          byVersion.set(version, { paths: [fullPath.join('>')], dev: isDev, optional: isOptional })
        } else {
          if (!isDev) info.dev = false
          if (!isOptional) info.optional = false
          info.paths.push(fullPath.join('>'))
        }
      }
      walk(next(), currentDepTypes, currentOptionalOnly, fullPath)
    }
  }

  for (const importerWalker of importerWalkers) {
    const importerSegment = importerWalker.importerId.replace(/\//g, '__')
    walk(importerWalker.step, depTypes, optionalOnly, [importerSegment])
  }

  if (opts.envLockfile) {
    const envLockfileObject = envLockfileToLockfileObject(opts.envLockfile)
    const envDepTypes = detectDepTypes(envLockfileObject)
    const envOptionalOnly = collectOptionalOnlyDepPaths(envLockfileObject)
    for (const { importerId, step } of lockfileWalkerGroupImporterSteps(envLockfileObject, Object.keys(envLockfileObject.importers) as ProjectId[], { include: opts.include })) {
      walk(step, envDepTypes, envOptionalOnly, [importerId])
    }
  }

  return paths
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
