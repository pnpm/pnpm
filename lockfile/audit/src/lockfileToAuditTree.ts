import path from 'path'
import { parse as parseDepPath, refToRelative, removeSuffix } from '@pnpm/dependency-path'
import type { EnvLockfile, LockfileObject, TarballResolution } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { lockfileWalkerGroupImporterSteps, type LockfileWalkerStep } from '@pnpm/lockfile.walker'
import { detectDepTypes, type DepTypes, DepType } from '@pnpm/lockfile.detect-dep-types'
import type { DependenciesField, ProjectId } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
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
    const { configDeps, packageManagerDeps } = envLockfileToAuditNodes(opts.envLockfile)
    if (Object.keys(configDeps).length > 0) {
      dependencies['configDependencies'] = wrapDepsGroup(configDeps)
    }
    if (Object.keys(packageManagerDeps).length > 0) {
      dependencies['packageManagerDependencies'] = wrapDepsGroup(packageManagerDeps)
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

function envLockfileToAuditNodes (envLockfile: EnvLockfile): {
  configDeps: Record<string, AuditNode>
  packageManagerDeps: Record<string, AuditNode>
} {
  const importer = envLockfile.importers['.']
  const visited = new Set<string>()
  const toAuditNodes = (deps: Record<string, { version: string }>): Record<string, AuditNode> => {
    const result: Record<string, AuditNode> = {}
    for (const [name, { version }] of Object.entries(deps)) {
      const depPath = refToRelative(version, name)
      if (depPath) {
        result[name] = envLockfileDepToAuditNode(envLockfile, depPath, visited)
      }
    }
    return result
  }
  return {
    configDeps: toAuditNodes(importer.configDependencies),
    packageManagerDeps: toAuditNodes(importer.packageManagerDependencies ?? {}),
  }
}

function envLockfileDepToAuditNode (
  envLockfile: EnvLockfile,
  depPath: string,
  visited: Set<string>
): AuditNode {
  const depPathWithoutSuffix = removeSuffix(depPath)
  const pkgInfo = envLockfile.packages[depPathWithoutSuffix] ?? envLockfile.packages[depPath]
  const snapshot = envLockfile.snapshots[depPath] ?? envLockfile.snapshots[depPathWithoutSuffix]
  const version = parseDepPath(depPathWithoutSuffix).version ?? depPath
  const node: AuditNode = {
    dev: false,
    integrity: (pkgInfo?.resolution as { integrity?: string } | undefined)?.integrity,
    version,
  }
  if (visited.has(depPath)) {
    return node
  }
  visited.add(depPath)
  const subdeps: Record<string, AuditNode> = {}
  const allSubDeps = { ...snapshot?.dependencies, ...snapshot?.optionalDependencies }
  for (const [depName, depVersion] of Object.entries(allSubDeps)) {
    const subDepPath = refToRelative(depVersion, depName)
    if (subDepPath) {
      subdeps[depName] = envLockfileDepToAuditNode(envLockfile, subDepPath, visited)
    }
  }
  if (Object.keys(subdeps).length > 0) {
    node.dependencies = subdeps
    node.requires = toRequires(subdeps)
  }
  return node
}
