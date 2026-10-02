import type { Dirent } from 'node:fs'
import fs from 'node:fs/promises'
import path from 'node:path'

import { isError } from '@pnpm/error'
import type { DepPath } from '@pnpm/types'
import { strict as isStrictSubdir } from 'is-subdir'

import type { DependenciesGraph, DependenciesGraphNode } from './buildGraph.js'

export interface RemoveIncompatibleOptionalOptions {
  enableGlobalVirtualStore?: boolean
  hoistedLocations?: Record<string, string[]>
  linkedModulesDirs?: string[]
  lockfileDir: string
  skipped?: Set<DepPath>
}

export async function removeIncompatibleOptional<NodeId extends string> (
  depPath: NodeId,
  depNode: DependenciesGraphNode<NodeId>,
  depGraph: DependenciesGraph<NodeId>,
  opts: RemoveIncompatibleOptionalOptions
): Promise<void> {
  depNode.installable = false
  opts.skipped?.add(depNode.depPath)
  if (opts.hoistedLocations != null) {
    const copies = opts.hoistedLocations[depNode.depPath] ?? []
    await removeAll([depNode.dir, ...copies.map((location) => path.join(opts.lockfileDir, location))])
    return
  }
  const removed = await linksTo(depNode.dir, opts.linkedModulesDirs ?? [])
  // A global virtual store slot, and the links between slots, are shared with
  // every other project that resolves to them.
  if (!opts.enableGlobalVirtualStore) {
    removed.push(depNode.dir, ...linksFromDependents(depPath, depGraph))
  }
  await removeAll(removed)
}

function linksFromDependents<NodeId extends string> (depPath: NodeId, depGraph: DependenciesGraph<NodeId>): string[] {
  const links: string[] = []
  for (const node of Object.values(depGraph) as Array<DependenciesGraphNode<NodeId>>) {
    for (const [alias, child] of Object.entries(node.children)) {
      if (child !== depPath) continue
      const link = containedNodeModulesLink(node.modules, alias)
      if (link != null) links.push(link)
    }
  }
  return links
}

async function removeAll (paths: string[]): Promise<void> {
  await Promise.all(paths.map(async (target) => fs.rm(target, { recursive: true, force: true })))
}

async function linksTo (target: string, modulesDirs: string[]): Promise<string[]> {
  const realTarget = await realpathOrUndefined(target)
  if (realTarget == null) return []
  const candidates = (await Promise.all(modulesDirs.map(listModulesDirEntries))).flat()
  const matches = await Promise.all(candidates.map(async (candidate) =>
    await realpathOrUndefined(candidate) === realTarget ? candidate : undefined
  ))
  return matches.filter((match): match is string => match != null)
}

/**
 * The links in a `node_modules` directory, including those inside scope
 * directories. A scope directory that is itself a link is not followed.
 */
async function listModulesDirEntries (modulesDir: string): Promise<string[]> {
  const entries = await readdirOrEmpty(modulesDir)
  const nested = await Promise.all(entries.map(async (entry) => {
    const entryPath = path.join(modulesDir, entry.name)
    if (entry.name.startsWith('@') && entry.isDirectory()) {
      return (await readdirOrEmpty(entryPath))
        .filter((scoped) => scoped.isSymbolicLink())
        .map((scoped) => path.join(entryPath, scoped.name))
    }
    return entry.isSymbolicLink() ? [entryPath] : []
  }))
  return nested.flat()
}

async function readdirOrEmpty (dir: string): Promise<Dirent[]> {
  try {
    return await fs.readdir(dir, { withFileTypes: true })
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) return []
    throw err
  }
}

async function realpathOrUndefined (target: string): Promise<string | undefined> {
  try {
    return await fs.realpath(target)
  } catch (err: unknown) {
    // A dangling or cyclic link, or one whose target runs through a regular
    // file, resolves to nothing, so it cannot point at the target.
    if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ELOOP' || err.code === 'ENOTDIR')) return undefined
    throw err
  }
}

function containedNodeModulesLink (modulesDir: string, alias: string): string | undefined {
  const nodeModulesDir = path.resolve(modulesDir)
  const link = path.resolve(nodeModulesDir, alias)
  const relative = path.relative(nodeModulesDir, link)
  if (
    relative === '' ||
    relative === '..' ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative)
  ) return undefined
  return link
}

/**
 * Remove every installed copy of an optional dependency whose build failed,
 * so a consumer that probes for it finds it absent rather than half-built.
 * The links to it in `linkedModulesDirs` are removed too, so the package is
 * absent rather than linked to nothing. The next install that has work to do
 * retries the build.
 * Under the global virtual store this removes the package directory of the
 * shared slot, whose lock the caller holds. The slot keeps its lock and its
 * dependency links, and every project that links it finds the package absent.
 * A hoisted location outside the lockfile directory is never removed.
 * Rejects if a removal fails, so the package is not reported as skipped.
 */
export async function removeSkippedOptionalDependency<NodeId extends string> (
  depNode: DependenciesGraphNode<NodeId>,
  opts: { hoistedLocations?: Record<string, string[]>, linkedModulesDirs?: string[], lockfileDir: string }
): Promise<void> {
  // Links are matched by their resolved target, so they are found before
  // the directory they point to is removed.
  const links = await linksTo(depNode.dir, opts.linkedModulesDirs ?? [])
  const dirs = new Set([
    depNode.dir,
    ...(opts.hoistedLocations?.[depNode.depPath] ?? [])
      .map((hoistedLocation) => path.join(opts.lockfileDir, hoistedLocation))
      .filter((dir) => isStrictSubdir(opts.lockfileDir, dir)),
  ])
  await removeAll(links)
  await Promise.all(Array.from(dirs, (dir) => fs.rm(dir, { recursive: true, force: true })))
}
