import { DepType, type DepTypes, detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import { convertToLockfileObject } from '@pnpm/lockfile.fs'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { lockfileWalkerGroupImporterSteps, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import type { DependenciesField, ProjectId } from '@pnpm/types'

export interface PathInfo {
  paths: string[]
  dev: boolean
}

// Versions installed per package name, keyed by version.
export type AuditPathIndex = Record<string, Map<string, PathInfo>>

export interface AuditIndexRequest {
  // Flat map suitable as the POST body for `/advisories/bulk`.
  request: Record<string, string[]>
  totalDependencies: number
  devDependencies: number
}

export interface AuditIndexOptions {
  envLockfile?: EnvLockfile | null
  include?: { [dependenciesField in DependenciesField]: boolean }
  // Pre-computed dep types. Callers that also call buildAuditPathIndex on the
  // same lockfile can share this to avoid walking the lockfile twice.
  depTypes?: DepTypes
}

export function lockfileToAuditRequest (
  lockfile: LockfileObject,
  opts: AuditIndexOptions
): AuditIndexRequest {
  const importerIds = Object.keys(lockfile.importers) as ProjectId[]
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, importerIds, { include: opts.include })
  const depTypes = opts.depTypes ?? detectDepTypes(lockfile)

  const request: Record<string, string[]> = {}
  const seenVersions: Record<string, Set<string>> = {}
  let totalDependencies = 0
  let devDependencies = 0

  const visit = (step: LockfileWalkerStep, currentDepTypes: DepTypes): void => {
    for (const { depPath, pkgSnapshot, next } of step.dependencies) {
      const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      if (version) {
        let versions = seenVersions[name]
        if (!versions) {
          versions = new Set()
          seenVersions[name] = versions
          request[name] = []
        }
        if (!versions.has(version)) {
          versions.add(version)
          request[name].push(version)
          totalDependencies++
          if (currentDepTypes[depPath] === DepType.DevOnly) {
            devDependencies++
          }
        }
      }
      visit(next(), currentDepTypes)
    }
  }

  for (const importerWalker of importerWalkers) {
    visit(importerWalker.step, depTypes)
  }
  if (opts.envLockfile) {
    const envLockfileObject = envLockfileToLockfileObject(opts.envLockfile)
    const envDepTypes = detectDepTypes(envLockfileObject)
    for (const { step } of lockfileWalkerGroupImporterSteps(envLockfileObject, Object.keys(envLockfileObject.importers) as ProjectId[], { include: opts.include })) {
      visit(step, envDepTypes)
    }
  }

  return { request, totalDependencies, devDependencies }
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

  const walk = (step: LockfileWalkerStep, currentDepTypes: DepTypes, trail: string[]): void => {
    for (const { depPath, pkgSnapshot, next } of step.dependencies) {
      const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      const fullPath = [...trail, name]
      if (version && vulnerableNames.has(name)) {
        const isDev = currentDepTypes[depPath] === DepType.DevOnly
        let byVersion = paths[name]
        if (!byVersion) {
          byVersion = new Map()
          paths[name] = byVersion
        }
        const info = byVersion.get(version)
        if (!info) {
          byVersion.set(version, { paths: [fullPath.join('>')], dev: isDev })
        } else {
          if (!isDev) {
            info.dev = false
          }
          info.paths.push(fullPath.join('>'))
        }
      }
      walk(next(), currentDepTypes, fullPath)
    }
  }

  for (const importerWalker of importerWalkers) {
    const importerSegment = importerWalker.importerId.replace(/\//g, '__')
    walk(importerWalker.step, depTypes, [importerSegment])
  }

  if (opts.envLockfile) {
    const envLockfileObject = envLockfileToLockfileObject(opts.envLockfile)
    const envDepTypes = detectDepTypes(envLockfileObject)
    for (const { importerId, step } of lockfileWalkerGroupImporterSteps(envLockfileObject, Object.keys(envLockfileObject.importers) as ProjectId[], { include: opts.include })) {
      walk(step, envDepTypes, [importerId])
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
