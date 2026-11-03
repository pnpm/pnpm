import fs from 'node:fs'
import path from 'node:path'

import { pkgRequiresBuild } from '@pnpm/building.pkg-requires-build'
import { createAllowBuildFunction } from '@pnpm/building.policy'
import { formatIntegrity } from '@pnpm/crypto.integrity'
import { iteratePkgsForVirtualStore, projectLinksMatchPackageDirs } from '@pnpm/deps.graph-builder'
import { PnpmError } from '@pnpm/error'
import { safeJoinWorkspaceModulesDir } from '@pnpm/fs.symlink-dependency'
import { getContextForSingleImporter } from '@pnpm/installing.context'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { packageIdFromSnapshot } from '@pnpm/lockfile.utils'
import { streamParser } from '@pnpm/logger'
import type {
  PackageFileInfo,
  PackageFilesIndex,
  SideEffectsDiff,
} from '@pnpm/store.cafs'
import type { TarballResolution } from '@pnpm/store.controller-types'
import { pickStoreIndexKey, StoreIndex } from '@pnpm/store.index'
import type { DepPath, ProjectId } from '@pnpm/types'
import dint from 'dint'
import pFilter from 'p-filter'

import {
  extendStoreStatusOptions,
  type StoreStatusOptions,
} from './extendStoreStatusOptions.js'

function getMapEntries<Value> (mapOrObj: Map<string, Value> | Record<string, Value> | undefined): Array<[string, Value]> {
  if (!mapOrObj) return []
  if (mapOrObj instanceof Map) {
    return Array.from(mapOrObj.entries())
  }
  return Object.entries(mapOrObj)
}

function getSideEffectsDiffs (sideEffects: Map<string, SideEffectsDiff> | Record<string, SideEffectsDiff> | undefined): SideEffectsDiff[] {
  if (!sideEffects) return []
  if (sideEffects instanceof Map) {
    return Array.from(sideEffects.values())
  }
  return Object.values(sideEffects)
}

function isIsolatedDir (targetDir: string, filePaths: string[]): boolean {
  if (!isRealDirectory(targetDir)) {
    return false
  }
  return filePaths.every((relPath) => isPathIsolated(targetDir, relPath))
}

function isRealDirectory (dirPath: string): boolean {
  try {
    return !fs.lstatSync(dirPath).isSymbolicLink()
  } catch (err: unknown) {
    if ((err as { code?: string })?.code === 'ENOENT') {
      return false
    }
    throw err
  }
}

function isPathIsolated (targetDir: string, relPath: string): boolean {
  const normalized = path.normalize(relPath)
  if (normalized === '..' || normalized.startsWith(`..${path.sep}`) || path.isAbsolute(normalized)) {
    return false
  }
  const parts = normalized.split(path.sep).filter(Boolean)
  let current = targetDir
  for (let partIndex = 0; partIndex < parts.length; partIndex++) {
    current = path.join(current, parts[partIndex])
    const isLeaf = partIndex === parts.length - 1
    const checkResult = checkStepIsolation(current, isLeaf)
    if (checkResult === 'break') {
      break
    }
    if (!checkResult) {
      return false
    }
  }
  return true
}

function checkStepIsolation (current: string, isLeaf: boolean): boolean | 'break' {
  try {
    const stat = fs.lstatSync(current)
    if (stat.isSymbolicLink()) {
      return false
    }
    if (isLeaf && stat.isFile() && stat.nlink > 1) {
      return false
    }
    return true
  } catch (err: unknown) {
    if ((err as { code?: string })?.code === 'ENOENT') {
      return 'break'
    }
    throw err
  }
}

interface PkgToCheck {
  depPath: DepPath
  id: string
  resolution: TarballResolution
  targetDir: string
  globalVirtualStore: boolean
}

export async function storeStatus (maybeOpts: StoreStatusOptions): Promise<string[]> {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  try {
    const opts = await extendStoreStatusOptions(maybeOpts)
    const ctx = await getContextForSingleImporter({}, {
      ...opts,
      extraBinPaths: [],
    })
    if (!ctx.wantedLockfile) return []

    const installedLockfile = opts.enableGlobalVirtualStore && ctx.existsCurrentLockfile ? ctx.currentLockfile : ctx.wantedLockfile
    const allowBuilds = ctx.modulesFile ? ctx.modulesFile.allowBuilds ?? {} : opts.allowBuilds
    const pkgs = Array.from(iteratePkgsForVirtualStore(installedLockfile, {
      ...opts,
      allowBuild: createAllowBuildFunction({ ...opts, allowBuilds }),
      virtualStoreDir: ctx.virtualStoreDir,
      globalVirtualStoreDir: opts.enableGlobalVirtualStore
        ? ctx.modulesFile?.virtualStoreDir ?? ctx.virtualStoreDir
        : path.join(ctx.storeDir, 'links'),
    }))
      .filter(({ pkgMeta }) => !ctx.skipped.has(pkgMeta.depPath))
      .map(({ pkgMeta: { depPath, pkgSnapshot, name }, dirInVirtualStore }) => ({
        depPath,
        globalVirtualStore: opts.enableGlobalVirtualStore === true,
        id: packageIdFromSnapshot(depPath, pkgSnapshot),
        resolution: pkgSnapshot.resolution as TarballResolution,
        targetDir: path.join(dirInVirtualStore, 'node_modules', name),
      }))
    if (opts.enableGlobalVirtualStore) {
      await validateGlobalStoreLinks(pkgs, { lockfile: installedLockfile, lockfileDir: opts.lockfileDir, modulesDir: opts.modulesDir ?? 'node_modules' })
    }
    return await findModifiedPackages(pkgs, ctx.storeDir)
  } finally {
    if ((reporter != null) && typeof reporter === 'function') {
      streamParser.removeListener('data', reporter)
    }
  }
}

async function validateGlobalStoreLinks (
  pkgs: PkgToCheck[],
  { lockfile, lockfileDir, modulesDir }: {
    lockfile: LockfileObject
    lockfileDir: string
    modulesDir: string
  }
): Promise<void> {
  const projects = Object.keys(lockfile.importers).map((id) => ({
    id: id as ProjectId,
    modulesDir: path.resolve(id === '.' ? lockfileDir : safeJoinWorkspaceModulesDir(lockfileDir, id), modulesDir),
  }))
  const packageDirs = new Map(pkgs.map((pkg) => [pkg.depPath, pkg.targetDir]))
  if (!await projectLinksMatchPackageDirs(projects, { lockfile, packageDirs })) {
    throw new PnpmError('STORE_STATUS_UNVERIFIABLE', 'Installed package links do not match the current global-store layout. Run "pnpm install" before checking store status.')
  }
}

async function findModifiedPackages (
  pkgs: PkgToCheck[],
  storeDir: string
): Promise<string[]> {
  const storeIndex = new StoreIndex(storeDir)
  try {
    const modified = await pFilter(pkgs, async (pkg) =>
      isPackageModified(storeIndex, pkg, pkg.targetDir), { concurrency: 8 })

    return modified.map(({ depPath }) => depPath)
  } finally {
    storeIndex.close()
  }
}

async function isPackageModified (
  storeIndex: StoreIndex,
  pkg: PkgToCheck,
  targetDir: string
): Promise<boolean> {
  const pkgIndexFilePath = pickStoreIndexKey(pkg.resolution, pkg.id, { built: true })
  const pkgFilesIndex = storeIndex.get(pkgIndexFilePath) as PackageFilesIndex | undefined
  if (!pkgFilesIndex) {
    return false
  }
  if (!fs.existsSync(targetDir)) {
    if (pkg.globalVirtualStore) {
      throw new PnpmError('STORE_STATUS_UNVERIFIABLE', `Cannot locate the installed package at ${targetDir}. Run "pnpm install" before checking store status.`)
    }
    return true
  }
  const fileEntries = getMapEntries<PackageFileInfo>(pkgFilesIndex.files)
  const dintFiles = toDintFiles(fileEntries, pkgFilesIndex.algo)
  if (await dint.check(targetDir, dintFiles)) {
    return false
  }
  if (await checkSideEffects(pkgFilesIndex, targetDir, fileEntries)) {
    return false
  }
  const requiresBuild =
    pkgFilesIndex.requiresBuild === true ||
    pkgRequiresBuild(pkgFilesIndex.manifest, pkgFilesIndex.files)
  if (requiresBuild && isIsolatedDir(targetDir, fileEntries.map(([filePath]) => filePath))) {
    return false
  }
  return true
}

function toDintFiles (
  fileEntries: Array<[string, PackageFileInfo]>,
  algo: string = 'sha512'
): Record<string, { integrity: string, size: number }> {
  const dintFiles: Record<string, { integrity: string, size: number }> = {}
  for (const [filePath, { digest, size }] of fileEntries) {
    dintFiles[filePath] = {
      integrity: formatIntegrity(algo, digest),
      size,
    }
  }
  return dintFiles
}

async function checkSideEffects (
  pkgFilesIndex: PackageFilesIndex,
  targetDir: string,
  fileEntries: Array<[string, PackageFileInfo]>
): Promise<boolean> {
  const sideEffectsDiffs = getSideEffectsDiffs(pkgFilesIndex.sideEffects)
  if (sideEffectsDiffs.length === 0) return false
  const sideEffectsChecks = await Promise.all(
    sideEffectsDiffs.map(async (diff) => {
      const sideEffectsDintFiles: Record<string, { integrity: string, size: number }> = {}
      const deleted = new Set(diff.deleted ?? [])
      for (const [filePath, { digest, size }] of fileEntries) {
        if (!deleted.has(filePath)) {
          sideEffectsDintFiles[filePath] = {
            integrity: formatIntegrity(pkgFilesIndex.algo, digest),
            size,
          }
        }
      }
      const addedEntries = getMapEntries<PackageFileInfo>(diff.added)
      for (const [filePath, { digest, size }] of addedEntries) {
        sideEffectsDintFiles[filePath] = {
          integrity: formatIntegrity(pkgFilesIndex.algo, digest),
          size,
        }
      }
      return dint.check(targetDir, sideEffectsDintFiles)
    })
  )
  return sideEffectsChecks.some(Boolean)
}

