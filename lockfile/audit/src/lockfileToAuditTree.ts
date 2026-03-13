import path from 'node:path'

import { DepType, type DepTypes, detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import { convertToLockfileObject } from '@pnpm/lockfile.fs'
import type { EnvLockfile, LockfileObject, TarballResolution } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { lockfileWalkerGroupImporterSteps, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import type { DependenciesField, ProjectId } from '@pnpm/types'
import { map as mapValues } from 'ramda'

export interface AuditNode {
  version?: string
  integrity?: string
  requires?: Record<string, string>
  dependencies?: { [name: string]: AuditNode }
  dev: boolean
}

export interface AuditTree extends AuditNode {
  name?: string
  install: string[]
  remove: string[]
  metadata: unknown
}

export async function lockfileToAuditTree (
  lockfile: LockfileObject,
  opts: {
    envLockfile?: EnvLockfile | null
    include?: { [dependenciesField in DependenciesField]: boolean }
    lockfileDir: string
  }
): Promise<AuditTree> {
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, Object.keys(lockfile.importers) as ProjectId[], { include: opts?.include })
  const dependencies: Record<string, AuditNode> = {}
  const depTypes = detectDepTypes(lockfile)
  await Promise.all(
    importerWalkers.map(async (importerWalker) => {
      const importerDeps = lockfileToAuditNode(depTypes, importerWalker.step)
      // For some reason the registry responds with 500 if the keys in dependencies have slashes
      // see issue: https://github.com/pnpm/pnpm/issues/2848
      const depName = importerWalker.importerId.replace(/\//g, '__')
      const manifest = await safeReadProjectManifestOnly(path.join(opts.lockfileDir, importerWalker.importerId))
      dependencies[depName] = {
        dependencies: importerDeps,
        dev: false,
        requires: toRequires(importerDeps),
        version: manifest?.version ?? '0.0.0',
      }
    })
  )
  if (opts.envLockfile) {
    const envLockfileObject = envLockfileToLockfileObject(opts.envLockfile)
    const envDepTypes = detectDepTypes(envLockfileObject)
    for (const { importerId, step } of lockfileWalkerGroupImporterSteps(envLockfileObject, Object.keys(envLockfileObject.importers) as ProjectId[], { include: opts.include })) {
      const deps = lockfileToAuditNode(envDepTypes, step)
      if (Object.keys(deps).length > 0) {
        dependencies[importerId] = wrapDepsGroup(deps)
      }
    }
  }
  const auditTree: AuditTree = {
    name: undefined,
    version: undefined,

    dependencies,
    dev: false,
    install: [],
    integrity: undefined,
    metadata: {},
    remove: [],
    requires: toRequires(dependencies),
  }
  return auditTree
}

function lockfileToAuditNode (depTypes: DepTypes, step: LockfileWalkerStep): Record<string, AuditNode> {
  const dependencies: Record<string, AuditNode> = {}
  for (const { depPath, pkgSnapshot, next } of step.dependencies) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    const subdeps = lockfileToAuditNode(depTypes, next())
    const dep: AuditNode = {
      dev: depTypes[depPath] === DepType.DevOnly,
      integrity: (pkgSnapshot.resolution as TarballResolution).integrity,
      version,
    }
    if (Object.keys(subdeps).length > 0) {
      dep.dependencies = subdeps
      dep.requires = toRequires(subdeps)
    }
    dependencies[name] = dep
  }
  return dependencies
}

function toRequires (auditNodesByDepName: Record<string, AuditNode>): Record<string, string> {
  return mapValues((auditNode) => auditNode.version!, auditNodesByDepName)
}

function wrapDepsGroup (deps: Record<string, AuditNode>): AuditNode {
  return {
    dependencies: deps,
    dev: false,
    requires: toRequires(deps),
    version: '0.0.0',
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
