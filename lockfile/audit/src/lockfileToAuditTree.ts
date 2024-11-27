import path from 'path'
import { type Lockfile, type TarballResolution } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { lockfileWalkerGroupImporterSteps, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import { detectDepTypes, type DepTypes, DepType } from '@pnpm/lockfile.detect-dep-types'
import { type DependenciesField, type ProjectId } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import mapValues from 'ramda/src/map'

export interface AuditNode {
  version?: string
  integrity?: string
  requires?: Record<string, string>
  dependencies?: { [name: string]: AuditNode }
  dev: boolean
}

export type AuditTree = AuditNode & {
  name?: string
  install: string[]
  remove: string[]
  metadata: unknown
}

export async function lockfileToAuditTree (
  lockfile: Lockfile,
  opts: {
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
