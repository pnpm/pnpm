import fs from 'node:fs'
import path from 'node:path'

import { getSlotShimNodePathOptions, linkBinsOfPackages } from '@pnpm/bins.linker'
import { isError } from '@pnpm/error'
import type { DependencyManifest } from '@pnpm/types'
import { loadJsonFile } from 'load-json-file'

const markerName = '.pnpm-gvs-bin-shims-v1'

export async function repairGlobalVirtualStoreBinShims (
  ctx: { extraNodePaths: string[], virtualStoreDir: string },
  opts: {
    enableGlobalVirtualStore?: boolean
    frozenStore?: boolean
    preferSymlinkedExecutables?: boolean
  }
): Promise<void> {
  if (!opts.enableGlobalVirtualStore || opts.frozenStore) return

  const markerPath = path.join(ctx.virtualStoreDir, markerName)
  if (await markerExists(markerPath)) return
  const rootEntries = await readDirectories(ctx.virtualStoreDir)
  if (rootEntries == null) return

  const slotShimNodePathOptions = getSlotShimNodePathOptions({
    enableGlobalVirtualStore: true,
    extraNodePaths: ctx.extraNodePaths,
  })
  await Promise.all(rootEntries
    .filter((entry) => entry !== 'node_modules')
    .map((entry) => repairRootEntry({
      rootPath: path.join(ctx.virtualStoreDir, entry),
      entry,
      slotShimNodePathOptions,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    })))
  await fs.promises.writeFile(markerPath, '1')
}

async function repairRootEntry (opts: {
  rootPath: string
  entry: string
  slotShimNodePathOptions: ReturnType<typeof getSlotShimNodePathOptions>
  preferSymlinkedExecutables?: boolean
}): Promise<void> {
  if (opts.entry === '@') {
    const packages = await readDirectories(opts.rootPath)
    await Promise.all((packages ?? []).map((packageName) => repairPackageSlots({
      packagePath: path.join(opts.rootPath, packageName),
      scope: '',
      ...opts,
    })))
    return
  }
  if (opts.entry.startsWith('@')) {
    const packages = await readDirectories(opts.rootPath)
    await Promise.all((packages ?? []).map((packageName) => repairPackageSlots({
      packagePath: path.join(opts.rootPath, packageName),
      scope: opts.entry,
      ...opts,
    })))
    return
  }
  await repairPackageSlots({ packagePath: opts.rootPath, scope: '', ...opts })
}

async function repairPackageSlots (opts: {
  packagePath: string
  scope: string
  slotShimNodePathOptions: ReturnType<typeof getSlotShimNodePathOptions>
  preferSymlinkedExecutables?: boolean
}): Promise<void> {
  const versions = await readDirectories(opts.packagePath)
  await Promise.all((versions ?? []).map((version) => repairVersionSlots({
    ...opts,
    versionPath: path.join(opts.packagePath, version),
  })))
}

async function repairVersionSlots (opts: {
  packagePath: string
  scope: string
  slotShimNodePathOptions: ReturnType<typeof getSlotShimNodePathOptions>
  preferSymlinkedExecutables?: boolean
  versionPath: string
}): Promise<void> {
  const hashes = await readDirectories(opts.versionPath)
  await Promise.all((hashes ?? []).map((hash) => repairSlot({
    ...opts,
    slotPath: path.join(opts.versionPath, hash),
  })))
}

async function repairSlot (opts: {
  packagePath: string
  scope: string
  slotShimNodePathOptions: ReturnType<typeof getSlotShimNodePathOptions>
  preferSymlinkedExecutables?: boolean
  slotPath: string
}): Promise<void> {
  const packageName = path.basename(opts.packagePath)
  const modulesDir = path.join(opts.slotPath, 'node_modules')
  const packageDir = opts.scope
    ? path.join(modulesDir, opts.scope, packageName)
    : path.join(modulesDir, packageName)
  if (!await pathExists(packageDir)) return

  const packages = await readPackageBins(modulesDir, packageDir)
  await linkBinsOfPackages(packages, path.join(packageDir, 'node_modules/.bin'), {
    ...opts.slotShimNodePathOptions,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
  })
}

async function readPackageBins (
  modulesDir: string,
  ownPackageDir: string
): Promise<Array<{ location: string, manifest: DependencyManifest }>> {
  const childPaths = await packageDirectories(modulesDir)
  const candidates = childPaths.filter((childPath) => path.resolve(childPath) !== path.resolve(ownPackageDir))
  const packages = await Promise.all(candidates.map(async (location) => ({
    location,
    manifest: await readPackageManifest(location),
  })))
  return packages.filter((pkg): pkg is { location: string, manifest: DependencyManifest } => pkg.manifest != null)
}

async function readPackageManifest (location: string): Promise<DependencyManifest | null> {
  try {
    return await loadJsonFile<DependencyManifest>(path.join(location, 'package.json'))
  } catch (error: unknown) {
    if (isMissingPath(error)) return null
    throw error
  }
}

async function readDirectories (directory: string): Promise<string[] | undefined> {
  try {
    return (await fs.promises.readdir(directory, { withFileTypes: true }))
      .filter((entry) => entry.isDirectory())
      .map((entry) => entry.name)
  } catch (error: unknown) {
    if (isMissingPath(error)) return undefined
    throw error
  }
}

async function packageDirectories (modulesDir: string): Promise<string[]> {
  let entries: fs.Dirent[]
  try {
    entries = await fs.promises.readdir(modulesDir, { withFileTypes: true })
  } catch (error: unknown) {
    if (isMissingPath(error)) return []
    throw error
  }
  const packagePaths = await Promise.all(entries
    .filter((entry) => !entry.name.startsWith('.'))
    .map((entry) => packagePathsForEntry(modulesDir, entry)))
  return packagePaths.flat()
}

async function packagePathsForEntry (modulesDir: string, entry: fs.Dirent): Promise<string[]> {
  const entryPath = path.join(modulesDir, entry.name)
  if (!entry.name.startsWith('@')) {
    return entry.isDirectory() || entry.isSymbolicLink() ? [entryPath] : []
  }
  const names = await packageEntries(entryPath)
  return names.map((name) => path.join(entryPath, name))
}

async function packageEntries (directory: string): Promise<string[]> {
  try {
    return (await fs.promises.readdir(directory, { withFileTypes: true }))
      .filter((entry) => entry.isDirectory() || entry.isSymbolicLink())
      .map((entry) => entry.name)
  } catch (error: unknown) {
    if (isMissingPath(error)) return []
    throw error
  }
}

async function markerExists (markerPath: string): Promise<boolean> {
  try {
    return (await fs.promises.readFile(markerPath, 'utf8')) === '1'
  } catch (error: unknown) {
    if (isMissingPath(error)) return false
    throw error
  }
}

async function pathExists (target: string): Promise<boolean> {
  try {
    await fs.promises.access(target)
    return true
  } catch (error: unknown) {
    if (isMissingPath(error)) return false
    throw error
  }
}

function isMissingPath (error: unknown): boolean {
  return isError(error) && 'code' in error &&
    (error.code === 'ENOENT' || error.code === 'ENOTDIR')
}
