import { promises as fs } from 'node:fs'

import type { DependenciesGraphNode } from '@pnpm/deps.graph-builder'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import { removeObsoleteDependency } from '@pnpm/installing.linking.modules-cleaner'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { symlinkAllModules } from '@pnpm/worker'
import { pickBy } from 'ramda'

import { limitModulesDirReads } from './limits.js'

type ModulesLinkNode = Pick<DependenciesGraphNode, 'children' | 'depPath' | 'optionalDependencies' | 'modules' | 'name'>

type PackageSnapshot = NonNullable<LockfileObject['packages']>[DependenciesGraphNode['depPath']]

interface ChangedChildrenOptions {
  currentLockfile?: LockfileObject | null
  relinkChangedDependenciesOnly?: boolean
  wantedLockfile: LockfileObject
}

export async function linkAllModules (
  depNodes: ModulesLinkNode[],
  opts: ChangedChildrenOptions & {
    optional: boolean
  }
): Promise<void> {
  const changes = await Promise.all(depNodes.map((depNode) => getChangedChildren(depNode, opts)))
  await Promise.all(changes.flatMap(({ depNode, removedAliases }) =>
    removedAliases.map((alias) => limitModulesDirReads(() => removeObsoleteDependency(depNode.modules, alias)))
  ))
  await symlinkAllModules({
    deps: changes.map(({ children, depNode }) => {
      return {
        children: opts.optional
          ? children
          : pickBy((_, childAlias) => !depNode.optionalDependencies.has(childAlias), children),
        modules: depNode.modules,
        name: depNode.name,
      }
    }),
  })
}

async function getChangedChildren (
  depNode: ModulesLinkNode,
  opts: ChangedChildrenOptions
): Promise<{
  children: Record<string, string>
  depNode: ModulesLinkNode
  removedAliases: string[]
}> {
  const currentSnapshot = opts.currentLockfile?.packages?.[depNode.depPath]
  const wantedSnapshot = opts.wantedLockfile.packages?.[depNode.depPath]
  if (currentSnapshot == null || wantedSnapshot == null) {
    return { children: depNode.children, depNode, removedAliases: [] }
  }
  const snapshots: SnapshotDependencies = {
    currentSnapshot,
    wantedSnapshot,
    currentDependencies: Object.assign(Object.create(null), currentSnapshot.dependencies, currentSnapshot.optionalDependencies) as Record<string, string>,
    wantedDependencies: Object.assign(Object.create(null), wantedSnapshot.dependencies, wantedSnapshot.optionalDependencies) as Record<string, string>,
  }
  const changedChildren = opts.relinkChangedDependenciesOnly
    ? await pickChangedChildren(depNode, snapshots)
    : depNode.children
  return {
    children: changedChildren,
    depNode,
    removedAliases: Object.keys(snapshots.currentDependencies).filter((alias) => alias !== depNode.name && !Object.hasOwn(snapshots.wantedDependencies, alias)),
  }
}

interface SnapshotDependencies {
  currentSnapshot: PackageSnapshot
  wantedSnapshot: PackageSnapshot
  currentDependencies: Record<string, string>
  wantedDependencies: Record<string, string>
}

async function pickChangedChildren (depNode: ModulesLinkNode, snapshots: SnapshotDependencies): Promise<Record<string, string>> {
  const changedEntries = await Promise.all(Object.entries(depNode.children).map(async ([alias, childDir]) =>
    await childLinkIsStale(depNode.modules, { alias, childDir }, snapshots) ? [alias, childDir] as const : null
  ))
  return Object.fromEntries(changedEntries.filter((entry): entry is readonly [string, string] => entry != null))
}

async function childLinkIsStale (
  modulesDir: string,
  { alias, childDir }: { alias: string, childDir: string },
  { currentSnapshot, wantedSnapshot, currentDependencies, wantedDependencies }: SnapshotDependencies
): Promise<boolean> {
  if (currentDependencies[alias] !== wantedDependencies[alias]) return true
  if (Object.hasOwn(currentSnapshot.optionalDependencies ?? {}, alias) !== Object.hasOwn(wantedSnapshot.optionalDependencies ?? {}, alias)) return true
  return !await limitModulesDirReads(() => dependencyLinkMatches(modulesDir, alias, childDir))
}

async function dependencyLinkMatches (modulesDir: string, alias: string, childDir: string): Promise<boolean> {
  const [linkTarget, expectedTarget] = await Promise.all([
    realpathOrNull(safeJoinModulesDir(modulesDir, alias)),
    realpathOrNull(childDir),
  ])
  return linkTarget != null && expectedTarget != null && linkTarget === expectedTarget
}

async function realpathOrNull (filePath: string): Promise<string | null> {
  try {
    return await fs.realpath(filePath)
  } catch (err: unknown) {
    if (
      typeof err === 'object' &&
      err != null &&
      'code' in err &&
      (err.code === 'ENOENT' || err.code === 'ENOTDIR' || err.code === 'ELOOP')
    ) {
      return null
    }
    throw err
  }
}
