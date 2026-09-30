import { LockfileMissingDependencyError } from '@pnpm/error'
import { DepType, type DepTypes, detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import { convertToLockfileObject } from '@pnpm/lockfile.fs'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { lockfileWalkerGroupImporterSteps, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import type { DepPath, ProjectId } from '@pnpm/types'

import type { AuditIndexOptions, AuditIndexRequest, AuditPathIndex } from './auditIndexTypes.js'
import { collectOptionalOnlyDepPaths } from './dependencyEdges.js'
import { walkForPaths } from './walkForPaths.js'

export type { AuditIndexOptions, AuditIndexRequest, AuditPathIndex, PathInfo } from './auditIndexTypes.js'
export { collectOptionalOnlyDepPaths } from './dependencyEdges.js'

interface VersionState {
  devOnly: boolean
  optionalOnly: boolean
}

interface DependencyOccurrence extends VersionState {
  name: string
  version: string
}

interface AuditRequestAccumulator {
  result: AuditIndexRequest
  // Per (name, version) classification. Counted as dev/optional only while
  // every observed occurrence is dev-only / optional-only; once a non-dev or
  // non-optional occurrence is seen, the flag is cleared and the counter
  // decremented.
  versionStatesByName: Record<string, Map<string, VersionState>>
}

interface DepClassification {
  depTypes: DepTypes
  optionalOnly: Set<DepPath>
}

export function lockfileToAuditRequest (
  lockfile: LockfileObject,
  opts: AuditIndexOptions
): AuditIndexRequest {
  const importerIds = Object.keys(lockfile.importers) as ProjectId[]
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, importerIds, {
    include: opts.include,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
  })
  const mainClassification: DepClassification = {
    depTypes: opts.depTypes ?? detectDepTypes(lockfile, opts),
    optionalOnly: opts.optionalOnly ?? collectOptionalOnlyDepPaths(lockfile, opts),
  }

  // Use null-prototype objects for records keyed by package names so a
  // hostile or unusual package name (e.g. "__proto__") cannot pollute the
  // prototype or overwrite inherited properties.
  const accumulator: AuditRequestAccumulator = {
    result: {
      request: Object.create(null),
      totalDependencies: 0,
      dependencies: 0,
      devDependencies: 0,
      optionalDependencies: 0,
    },
    versionStatesByName: Object.create(null),
  }

  for (const importerWalker of importerWalkers) {
    registerStepOccurrences(accumulator, mainClassification, importerWalker.step)
  }
  if (opts.envLockfile) {
    registerEnvLockfileOccurrences(accumulator, opts.envLockfile, opts.include)
  }

  return accumulator.result
}

function registerEnvLockfileOccurrences (
  accumulator: AuditRequestAccumulator,
  envLockfile: EnvLockfile,
  include: AuditIndexOptions['include']
): void {
  const envLockfileObject = envLockfileToLockfileObject(envLockfile)
  const envClassification: DepClassification = {
    depTypes: detectDepTypes(envLockfileObject),
    optionalOnly: collectOptionalOnlyDepPaths(envLockfileObject, { include }),
  }
  for (const { step } of lockfileWalkerGroupImporterSteps(envLockfileObject, Object.keys(envLockfileObject.importers) as ProjectId[], { include })) {
    registerStepOccurrences(accumulator, envClassification, step)
  }
}

// The walker already de-duplicates by depPath internally, so we don't need a
// second visited set here. An explicit frame stack stands in for recursion
// (registering each dependency before descending, preserving pre-order) so a
// deep dependency chain from an untrusted lockfile cannot overflow the call
// stack.
function registerStepOccurrences (
  accumulator: AuditRequestAccumulator,
  classification: DepClassification,
  rootStep: LockfileWalkerStep
): void {
  assertNoMissingDependency(rootStep)
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
      registerOccurrence(accumulator, {
        name,
        version,
        devOnly: classification.depTypes[depPath] === DepType.DevOnly,
        optionalOnly: classification.optionalOnly.has(depPath),
      })
    }
    const nextStep = next()
    assertNoMissingDependency(nextStep)
    stack.push({ dependencies: nextStep.dependencies, next: 0 })
  }
}

function assertNoMissingDependency (step: LockfileWalkerStep): void {
  if (step.missing.length > 0) {
    throw new LockfileMissingDependencyError(step.missing[0])
  }
}

function registerOccurrence (accumulator: AuditRequestAccumulator, occurrence: DependencyOccurrence): void {
  let versionStates = accumulator.versionStatesByName[occurrence.name]
  if (!versionStates) {
    versionStates = new Map()
    accumulator.versionStatesByName[occurrence.name] = versionStates
    accumulator.result.request[occurrence.name] = []
  }
  const state = versionStates.get(occurrence.version)
  if (!state) {
    versionStates.set(occurrence.version, { devOnly: occurrence.devOnly, optionalOnly: occurrence.optionalOnly })
    countNewVersion(accumulator.result, occurrence)
    return
  }
  reclassifyVersion(accumulator.result, state, occurrence)
}

function countNewVersion (result: AuditIndexRequest, occurrence: DependencyOccurrence): void {
  result.request[occurrence.name].push(occurrence.version)
  result.totalDependencies++
  if (occurrence.devOnly) result.devDependencies++
  if (occurrence.optionalOnly) result.optionalDependencies++
  if (!occurrence.devOnly && !occurrence.optionalOnly) result.dependencies++
}

function reclassifyVersion (result: AuditIndexRequest, state: VersionState, occurrence: DependencyOccurrence): void {
  const wasProduction = !state.devOnly && !state.optionalOnly
  if (state.devOnly && !occurrence.devOnly) {
    state.devOnly = false
    result.devDependencies--
  }
  if (state.optionalOnly && !occurrence.optionalOnly) {
    state.optionalOnly = false
    result.optionalDependencies--
  }
  if (!wasProduction && !state.devOnly && !state.optionalOnly) {
    result.dependencies++
  }
}

export function buildAuditPathIndex (
  lockfile: LockfileObject,
  vulnerableNames: Set<string>,
  opts: AuditIndexOptions
): AuditPathIndex {
  // Null-prototype record keyed by package name to avoid prototype pollution
  // from registry-supplied or lockfile-supplied names.
  const paths: AuditPathIndex = Object.create(null)
  const depTypes = opts.depTypes ?? detectDepTypes(lockfile, opts)
  const optionalOnly = opts.optionalOnly ?? collectOptionalOnlyDepPaths(lockfile, opts)

  walkForPaths({
    lockfile,
    vulnerableNames,
    paths,
    depTypes,
    optionalOnly,
    include: opts.include,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    importerSegmentOf: (importerId) => importerId.replace(/\//g, '__'),
  })

  if (opts.envLockfile) {
    const envLockfileObject = envLockfileToLockfileObject(opts.envLockfile)
    walkForPaths({
      lockfile: envLockfileObject,
      vulnerableNames,
      paths,
      depTypes: detectDepTypes(envLockfileObject),
      optionalOnly: collectOptionalOnlyDepPaths(envLockfileObject, { include: opts.include }),
      include: opts.include,
      importerSegmentOf: (importerId) => importerId,
    })
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
