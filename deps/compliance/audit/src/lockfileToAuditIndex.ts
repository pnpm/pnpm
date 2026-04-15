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
  // Reachable (name, version) pairs and their dev status, used for the second walk
  reachable: Record<string, Map<string, { dev: boolean }>>
}

export interface AuditIndexOptions {
  envLockfile?: EnvLockfile | null
  include?: { [dependenciesField in DependenciesField]: boolean }
}

export function lockfileToAuditRequest (
  lockfile: LockfileObject,
  opts: AuditIndexOptions
): AuditIndexRequest {
  const importerIds = Object.keys(lockfile.importers) as ProjectId[]
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, importerIds, { include: opts.include })
  const depTypes = detectDepTypes(lockfile)

  const counts = { total: 0, dev: 0 }
  const reachable: AuditIndexRequest['reachable'] = {}

  const walkForRequest = (step: LockfileWalkerStep, currentDepTypes: DepTypes) => {
    for (const { depPath, pkgSnapshot, next } of step.dependencies) {
      const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      if (version) {
        counts.total++
        const isDev = currentDepTypes[depPath] === DepType.DevOnly
        if (isDev) {
          counts.dev++
        }
        let byVersion = reachable[name]
        if (!byVersion) {
          byVersion = new Map()
          reachable[name] = byVersion
        }
        const info = byVersion.get(version)
        if (!info) {
          byVersion.set(version, { dev: isDev })
        } else if (!isDev) {
          info.dev = false
        }
      }
      walkForRequest(next(), currentDepTypes)
    }
  }

  for (const importerWalker of importerWalkers) {
    walkForRequest(importerWalker.step, depTypes)
  }
  if (opts.envLockfile) {
    const envLockfileObject = envLockfileToLockfileObject(opts.envLockfile)
    const envDepTypes = detectDepTypes(envLockfileObject)
    for (const { step } of lockfileWalkerGroupImporterSteps(envLockfileObject, Object.keys(envLockfileObject.importers) as ProjectId[], { include: opts.include })) {
      walkForRequest(step, envDepTypes)
    }
  }

  const request: Record<string, string[]> = {}
  for (const [name, versions] of Object.entries(reachable)) {
    request[name] = Array.from(versions.keys())
  }

  return {
    request,
    totalDependencies: counts.total,
    devDependencies: counts.dev,
    reachable,
  }
}

export function buildAuditPathIndex (
  lockfile: LockfileObject,
  vulnerableNames: Set<string>,
  opts: AuditIndexOptions
): AuditPathIndex {
  const paths: AuditPathIndex = {}
  const importerIds = Object.keys(lockfile.importers) as ProjectId[]
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, importerIds, { include: opts.include })
  const depTypes = detectDepTypes(lockfile)

  const walk = (step: LockfileWalkerStep, currentDepTypes: DepTypes, trail: string[]) => {
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
