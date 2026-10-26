import path from 'node:path'

import { LOCKFILE_VERSION } from '@pnpm/constants'
import { isError, PnpmError } from '@pnpm/error'
import { type Modules, readModulesManifest } from '@pnpm/installing.modules-yaml'
import { readCurrentLockfile } from '@pnpm/lockfile.fs'
import type { LockfileObject, LockfileResolution } from '@pnpm/lockfile.types'
import { packageIdFromSnapshot } from '@pnpm/lockfile.utils'
import { globalWarn } from '@pnpm/logger'
import { getFilePathByModeInCafs, type PackageFileInfo, type PackageFilesIndex } from '@pnpm/store.cafs'
import { gitHostedStoreIndexKey, pickStoreIndexKey, type StoreIndex, storeIndexKey } from '@pnpm/store.index'
import type { DepPath } from '@pnpm/types'
import { readWorkspaceManifestKeys } from '@pnpm/workspace.workspace-manifest-reader'

import { getRegisteredProjects } from './projectRegistry.js'

/**
 * Every file of the store index rows of the packages in each registered
 * project's installed lockfile, including the files of every side-effects
 * entry. A package imported by copy or clone, such as a patched or built
 * one, has no hard link to the store, so the prune needs this list to keep
 * it. A project without an installed lockfile, a project whose lockfile
 * version this pnpm does not support, and a package without a row add
 * nothing.
 */
export async function getInUseFiles (storeDir: string, storeIndex: StoreIndex): Promise<Set<string>> {
  const { keys, binaryKeys } = await registeredProjectsIndexKeys(storeDir)
  for (const key of manifestIndexKeys(storeIndex, storeDir, binaryKeys)) keys.add(key)
  const files = new Set<string>()
  for (const key of keys) {
    const row = readIndexRow(storeIndex, storeDir, key)
    if (row == null) continue
    for (const { digest, mode } of rowFiles(row)) {
      files.add(path.normalize(getFilePathByModeInCafs(storeDir, digest, mode)))
    }
  }
  return files
}

interface IndexKey {
  key: string
  binary: boolean
}

async function registeredProjectsIndexKeys (storeDir: string): Promise<{ keys: Set<string>, binaryKeys: Set<string> }> {
  const keys = new Set<string>()
  const binaryKeys = new Set<string>()
  for (const projectDir of await getRegisteredProjects(storeDir)) {
    // eslint-disable-next-line no-await-in-loop -- hold one project's parsed lockfiles at a time
    for (const { key, binary } of await projectIndexKeys(projectDir)) {
      keys.add(key)
      if (binary) binaryKeys.add(key)
    }
  }
  return { keys, binaryKeys }
}

async function projectIndexKeys (projectDir: string): Promise<IndexKey[]> {
  return (await readInstalledLockfiles(projectDir)).flatMap(lockfileIndexKeys)
}

function lockfileIndexKeys (lockfile: LockfileObject): IndexKey[] {
  return Object.entries(lockfile.packages ?? {}).flatMap(([depPath, snapshot]) =>
    packageIndexKeys(snapshot.resolution, packageIdFromSnapshot(depPath as DepPath, snapshot))
  )
}

/**
 * pnpm 12 shares this store and keys the row of a runtime as
 * `package-manifest\t<manifest integrity>\t<binary key>`. The lockfile has
 * no manifest integrity, so this scans the keys, but only when the
 * lockfiles name a binary.
 */
function manifestIndexKeys (storeIndex: StoreIndex, storeDir: string, binaryKeys: Set<string>): string[] {
  if (binaryKeys.size === 0) return []
  const prefix = 'package-manifest\t'
  const keys: string[] = []
  try {
    for (const key of storeIndex.keys()) {
      if (!key.startsWith(prefix)) continue
      const separator = key.indexOf('\t', prefix.length)
      if (separator !== -1 && binaryKeys.has(key.slice(separator + 1))) keys.push(key)
    }
  } catch (err: unknown) {
    throw inUseFilesError(storeDir, err)
  }
  return keys
}

function readIndexRow (storeIndex: StoreIndex, storeDir: string, key: string): PackageFilesIndex | undefined {
  try {
    return storeIndex.get(key) as PackageFilesIndex | undefined
  } catch (err: unknown) {
    throw inUseFilesError(storeDir, err)
  }
}

/**
 * The lockfiles the last install wrote, which record what is installed in
 * the project.
 *
 * The install writes the lockfile to `<modulesDir>/.pnpm`, or to the
 * `virtualStoreDir` that `<modulesDir>/.modules.yaml` records when the
 * project has no wanted lockfile. Under a global virtual store, that
 * setting names the shared store. The manifest does not say which case
 * applies, so the prune reads both.
 */
async function readInstalledLockfiles (projectDir: string): Promise<LockfileObject[]> {
  const modulesDir = path.resolve(projectDir, await readConfiguredModulesDir(projectDir))
  const defaultDir = path.join(modulesDir, '.pnpm')
  const modules = await readModulesManifestOfProject(modulesDir)
  const installStateDirs = new Set([defaultDir, modules?.virtualStoreDir ?? defaultDir])
  const lockfiles = await Promise.all(Array.from(installStateDirs, async (installStateDir) => readInstalledLockfile(projectDir, installStateDir)))
  return lockfiles.filter((lockfile) => lockfile != null)
}

/**
 * The `modulesDir` that the project's `pnpm-workspace.yaml` sets, or
 * `node_modules`. A `modulesDir` set only on the command line, in the
 * environment, or in the global config is not seen.
 */
async function readConfiguredModulesDir (projectDir: string): Promise<string> {
  try {
    // `PnpmSettings` does not declare `modulesDir`.
    const manifest = await readWorkspaceManifestKeys(projectDir, MODULES_DIR_SETTING) as Record<string, unknown> | undefined
    const modulesDir = manifest?.modulesDir
    return typeof modulesDir === 'string' ? modulesDir : 'node_modules'
  } catch (err: unknown) {
    throw inUseFilesError(projectDir, err)
  }
}

const MODULES_DIR_SETTING: ReadonlySet<string> = new Set(['modulesDir'])

async function readModulesManifestOfProject (modulesDir: string): Promise<Modules | null> {
  try {
    return await readModulesManifest(modulesDir)
  } catch (err: unknown) {
    throw inUseFilesError(modulesDir, err)
  }
}

/**
 * A lockfile of a version this pnpm cannot read adds nothing, like a
 * missing one, so one project installed by another pnpm does not stop the
 * prune. The hard-link check then decides for that project's files.
 */
async function readInstalledLockfile (projectDir: string, installStateDir: string): Promise<LockfileObject | null> {
  try {
    return await readCurrentLockfile(installStateDir, { ignoreIncompatible: false, wantedVersions: [LOCKFILE_VERSION] })
  } catch (err: unknown) {
    const lockfileVersion = unsupportedLockfileVersion(err)
    if (lockfileVersion != null) {
      globalWarn(`Skipped ${projectDir} because it was installed with lockfileVersion ${lockfileVersion}, which this version of pnpm does not support`)
      return null
    }
    throw inUseFilesError(installStateDir, err)
  }
}

function unsupportedLockfileVersion (err: unknown): string | undefined {
  if (!isError(err) || !('code' in err) || err.code !== 'ERR_PNPM_LOCKFILE_BREAKING_CHANGE') return undefined
  return (err as { lockfileVersion?: string }).lockfileVersion
}

function inUseFilesError (location: string, err: unknown): PnpmError {
  return new PnpmError('STORE_DIR_PRUNE_IN_USE_FILES',
    `Failed to read the store files that registered projects use: ${location}: ${isError(err) ? err.message : String(err)}`)
}

/**
 * The lockfile does not say which key the install used for a git-hosted
 * package, which has a key for its contents with and without a build, or for
 * a variations resolution, whose variant depends on the installing host
 * and its `supportedArchitectures`.
 */
function packageIndexKeys (resolution: LockfileResolution, pkgId: string): IndexKey[] {
  if (!('type' in resolution) || resolution.type == null) {
    if ('tarball' in resolution) {
      return [true, false].map((built) => packageKey(pickStoreIndexKey(resolution, pkgId, { built })))
    }
    return [packageKey(storeIndexKey(resolution.integrity, pkgId))]
  }
  switch (resolution.type) {
    case 'git':
      return [true, false].map((built) => packageKey(gitHostedStoreIndexKey(pkgId, { built })))
    case 'binary':
      return [{ key: storeIndexKey(resolution.integrity, pkgId), binary: true }]
    case 'variations':
      return resolution.variants.flatMap((variant) => packageIndexKeys(variant.resolution, pkgId))
    default:
      // A directory has no row, and the content of a custom one is not
      // known here.
      return []
  }
}

function packageKey (key: string): IndexKey {
  return { key, binary: false }
}

function * rowFiles (row: PackageFilesIndex): Iterable<PackageFileInfo> {
  yield * row.files.values()
  for (const sideEffectsDiff of row.sideEffects?.values() ?? []) {
    if (sideEffectsDiff.added != null) yield * sideEffectsDiff.added.values()
  }
}
