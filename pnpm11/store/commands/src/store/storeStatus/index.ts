import fs from 'node:fs'
import path from 'node:path'

import { pkgRequiresBuild } from '@pnpm/building.pkg-requires-build'
import { formatIntegrity } from '@pnpm/crypto.integrity'
import * as dp from '@pnpm/deps.path'
import { getContextForSingleImporter } from '@pnpm/installing.context'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  type PackageSnapshot,
} from '@pnpm/lockfile.utils'
import { streamParser } from '@pnpm/logger'
import type {
  PackageFileInfo,
  PackageFilesIndex,
  SideEffectsDiff,
} from '@pnpm/store.cafs'
import type { TarballResolution } from '@pnpm/store.controller-types'
import { pickStoreIndexKey, StoreIndex } from '@pnpm/store.index'
import type { DepPath } from '@pnpm/types'
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
  pkgPath: string
  name: string
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

    const pkgs = extractPackagesToCheck(ctx.wantedLockfile, ctx.skipped)
    return await findModifiedPackages(pkgs, ctx.storeDir, ctx.virtualStoreDir, opts.virtualStoreDirMaxLength)
  } finally {
    if ((reporter != null) && typeof reporter === 'function') {
      streamParser.removeListener('data', reporter)
    }
  }
}

function extractPackagesToCheck (
  wantedLockfile: NonNullable<Awaited<ReturnType<typeof getContextForSingleImporter>>['wantedLockfile']>,
  skipped: Set<string>
): PkgToCheck[] {
  return (Object.entries(wantedLockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>)
    .filter(([depPath]) => !skipped.has(depPath))
    .map(([depPath, pkgSnapshot]) => ({
      depPath,
      id: packageIdFromSnapshot(depPath, pkgSnapshot),
      resolution: pkgSnapshot.resolution as TarballResolution,
      pkgPath: depPath,
      ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
    }))
}

async function findModifiedPackages (
  pkgs: PkgToCheck[],
  storeDir: string,
  virtualStoreDir: string,
  virtualStoreDirMaxLength: number
): Promise<string[]> {
  const storeIndex = new StoreIndex(storeDir)
  try {
    const modified = await pFilter(pkgs, async (pkg) => {
      const targetDir = path.join(
        virtualStoreDir,
        dp.depPathToFilename(pkg.depPath, virtualStoreDirMaxLength),
        'node_modules',
        pkg.name
      )
      return isPackageModified(storeIndex, pkg, targetDir)
    }, { concurrency: 8 })

    return modified.map(({ pkgPath }) => pkgPath)
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

