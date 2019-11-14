import fetch from '@pnpm/fetch'
import { Lockfile } from '@pnpm/lockfile-types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { lockfileWalkerGroupImporterSteps, LockfileWalkerStep } from '@pnpm/lockfile-walker'
import { AuditReport } from './types'

export * from './types'

export type AuditNode = {
  version?: string
  integrity?: string
  requires: Object
  dependencies: { [name: string]: AuditNode }
  dev: boolean
}

export type AuditTree = AuditNode & {
  name?: string,
  install: Array<string>
  remove: Array<string>
  metadata: Object
}

export default async function audit (lockfile: Lockfile, opts: { registry: string }) {
  const auditTree = lockfileToAuditTree(lockfile)
  const registry = opts.registry.endsWith('/') ? opts.registry : `${opts.registry}/`
  const res = await fetch(`${registry}-/npm/v1/security/audits`, {
    body: JSON.stringify(auditTree),
    headers: { 'Content-Type': 'application/json' },
    method: 'post',
  })
  return res.json() as Promise<AuditReport>
}

function lockfileToAuditTree (lockfile: Lockfile): AuditTree {
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, Object.keys(lockfile.importers))
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
  for (const { relDepPath, pkgSnapshot, next } of step.dependencies) {
    const { name, version } = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
    const subdeps = lockfileToAuditNode(next())
    dependencies[name] = {
      dependencies: subdeps,
      dev: pkgSnapshot.dev,
      integrity: pkgSnapshot.resolution['integrity'],
      requires: toRequires(subdeps),
      version,
    }
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
