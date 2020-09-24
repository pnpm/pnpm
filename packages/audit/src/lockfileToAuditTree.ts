import { Lockfile } from '@pnpm/lockfile-types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { lockfileWalkerGroupImporterSteps, LockfileWalkerStep } from '@pnpm/lockfile-walker'
import { DependenciesField } from '@pnpm/types'

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
  metadata: Object
}

export default function lockfileToAuditTree (
  lockfile: Lockfile,
  opts?: {
    include?: { [dependenciesField in DependenciesField]: boolean }
  }
): AuditTree {
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, Object.keys(lockfile.importers), { include: opts?.include })
  const dependencies = {}
  importerWalkers.forEach((importerWalker) => {
    const importerDeps = lockfileToAuditNode(importerWalker.step)
    dependencies[importerWalker.importerId] = {
      dependencies: importerDeps,
      requires: toRequires(importerDeps),
      version: '0.0.0',
    }
  })
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

function lockfileToAuditNode (step: LockfileWalkerStep) {
  const dependencies = {}
  for (const { depPath, pkgSnapshot, next } of step.dependencies) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    const subdeps = lockfileToAuditNode(next())
    const dep: AuditNode = {
      dev: pkgSnapshot.dev === true,
      integrity: pkgSnapshot.resolution['integrity'],
      version,
    }
    if (Object.keys(subdeps).length) {
      dep.dependencies = subdeps
      dep.requires = toRequires(subdeps)
    }
    dependencies[name] = dep
  }
  return dependencies
}

function toRequires (auditNodesByDepName: Record<string, AuditNode>) {
  const requires = {}
  for (const subdepName of Object.keys(auditNodesByDepName)) {
    requires[subdepName] = auditNodesByDepName[subdepName].version
  }
  return requires
}
