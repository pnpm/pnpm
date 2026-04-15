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

// Versions installed per package name, keyed by version. For each (name, version)
// we also keep the set of install paths (e.g. `.>karma>http-proxy`) and whether
// every occurrence was dev-only.
export type AuditPathIndex = Record<string, Map<string, PathInfo>>

export interface AuditIndex {
  // Flat map suitable as the POST body for `/advisories/bulk`.
  request: Record<string, string[]>
  // Path information keyed by package name, used to populate
  // `findings[].paths` and dev/total dependency counts.
  paths: AuditPathIndex
}

export function lockfileToAuditIndex (
  lockfile: LockfileObject,
  opts: {
    envLockfile?: EnvLockfile | null
    include?: { [dependenciesField in DependenciesField]: boolean }
  }
): AuditIndex {
  const paths: AuditPathIndex = {}
  const importerIds = Object.keys(lockfile.importers) as ProjectId[]
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, importerIds, { include: opts.include })
  const depTypes = detectDepTypes(lockfile)
  for (const importerWalker of importerWalkers) {
    // Workspace importer ids may contain slashes (e.g. `packages/foo`). The
    // paths string uses `>` as the separator, so keep the importer segment
    // readable by replacing slashes with `__` like the legacy tree did.
    const importerSegment = importerWalker.importerId.replace(/\//g, '__')
    collectFromStep(paths, depTypes, importerWalker.step, [importerSegment])
  }
  if (opts.envLockfile) {
    const envLockfileObject = envLockfileToLockfileObject(opts.envLockfile)
    const envDepTypes = detectDepTypes(envLockfileObject)
    for (const { importerId, step } of lockfileWalkerGroupImporterSteps(envLockfileObject, Object.keys(envLockfileObject.importers) as ProjectId[], { include: opts.include })) {
      collectFromStep(paths, envDepTypes, step, [importerId])
    }
  }

  const request: Record<string, string[]> = {}
  for (const [name, byVersion] of Object.entries(paths)) {
    request[name] = [...byVersion.keys()]
  }
  return { request, paths }
}

function collectFromStep (paths: AuditPathIndex, depTypes: DepTypes, step: LockfileWalkerStep, trail: string[]): void {
  for (const { depPath, pkgSnapshot, next } of step.dependencies) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    if (version) {
      const isDev = depTypes[depPath] === DepType.DevOnly
      let byVersion = paths[name]
      if (!byVersion) {
        byVersion = new Map()
        paths[name] = byVersion
      }
      let info = byVersion.get(version)
      if (!info) {
        info = { paths: [], dev: isDev }
        byVersion.set(version, info)
      } else if (!isDev) {
        info.dev = false
      }
      info.paths.push([...trail, name].join('>'))
    }
    collectFromStep(paths, depTypes, next(), [...trail, name])
  }
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
