import path from 'path'
import { type LockfileObject, type TarballResolution } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { lockfileWalkerGroupImporterSteps, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import { detectDepTypes, type DepTypes, DepType } from '@pnpm/lockfile.detect-dep-types'
import { type DependenciesField, type ProjectId, type DepPath } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'

export interface BulkAuditNode {
  isImporter?: true
  isDirect: boolean
  name: string
  depPath?: DepPath
  version: string
  integrity?: string
  dependencies?: { [name: string]: BulkAuditNode }
  dependents: Set<BulkAuditNode>
  dev: boolean
}

export interface BulkAuditTree {
  importers: Map<string, BulkAuditNode>
  allNodesByPackageName: Map<string, Set<BulkAuditNode>>
}

export async function lockfileToBulkAuditTree (
  lockfile: LockfileObject,
  opts: {
    include?: { [dependenciesField in DependenciesField]: boolean }
    lockfileDir: string
  }
): Promise<BulkAuditTree> {
  const importerWalkers = lockfileWalkerGroupImporterSteps(lockfile, Object.keys(lockfile.importers) as ProjectId[], { include: opts?.include })
  const importerNodes = new Map<string, BulkAuditNode>()
  const depTypes = detectDepTypes(lockfile)
  const allNodesByPackageName = new Map<string, Set<BulkAuditNode>>()
  await Promise.all(
    importerWalkers.map(async (importerWalker) => {
      const importerDeps = lockfileToBulkAuditNode(depTypes, importerWalker.step, true, allNodesByPackageName)
      const manifest = await safeReadProjectManifestOnly(path.join(opts.lockfileDir, importerWalker.importerId))
      const importerNode: BulkAuditNode = {
        name: importerWalker.importerId,
        isImporter: true,
        isDirect: true,
        dependencies: importerDeps,
        dev: false,
        version: manifest?.version ?? '0.0.0',
        dependents: new Set(),
      }
      for (const dep of Object.values(importerDeps)) {
        dep.dependents.add(importerNode)
      }
      importerNodes.set(importerWalker.importerId, importerNode)
    })
  )
  const auditTree: BulkAuditTree = {
    importers: importerNodes,
    allNodesByPackageName,
  }
  return auditTree
}

function lockfileToBulkAuditNode (depTypes: DepTypes, step: LockfileWalkerStep, isDirect: boolean, allNodesByPackageName: Map<string, Set<BulkAuditNode>>): Record<string, BulkAuditNode> {
  const dependencies: Record<string, BulkAuditNode> = {}
  for (const { depPath, pkgSnapshot, next } of step.dependencies) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    const subdeps = lockfileToBulkAuditNode(depTypes, next(), false, allNodesByPackageName)
    const dep: BulkAuditNode = {
      isDirect,
      name,
      depPath,
      dev: depTypes[depPath] === DepType.DevOnly,
      integrity: (pkgSnapshot.resolution as TarballResolution).integrity,
      version,
      dependents: new Set(),
    }
    if (Object.keys(subdeps).length > 0) {
      dep.dependencies = subdeps
      for (const subdep of Object.values(subdeps)) {
        subdep.dependents.add(dep)
      }
    }
    dependencies[name] = dep
    let nodesByName = allNodesByPackageName.get(name)
    if (nodesByName) {
      nodesByName.add(dep)
    } else {
      nodesByName = new Set([dep])
      allNodesByPackageName.set(name, nodesByName)
    }
  }
  return dependencies
}
