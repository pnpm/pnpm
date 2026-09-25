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
import { pickStoreIndexKey } from '@pnpm/store.index'
import { StoreIndex } from '@pnpm/store.index'
import type { DepPath } from '@pnpm/types'
import dint from 'dint'
import pFilter from 'p-filter'

import {
  extendStoreStatusOptions,
  type StoreStatusOptions,
} from './extendStoreStatusOptions.js'

function getMapEntries<V> (mapOrObj: Map<string, V> | Record<string, V> | undefined): Array<[string, V]> {
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
  try {
    const dirStat = fs.lstatSync(targetDir)
    if (dirStat.isSymbolicLink()) {
      return false
    }
  } catch (err: unknown) {
    if ((err as { code?: string })?.code === 'ENOENT') {
      return false
    }
    throw err
  }

  for (const relPath of filePaths) {
    const normalized = path.normalize(relPath)
    if (normalized === '..' || normalized.startsWith(`..${path.sep}`) || path.isAbsolute(normalized)) {
      return false
    }
    const parts = normalized.split(path.sep).filter(Boolean)
    let current = targetDir
    for (let i = 0; i < parts.length; i++) {
      current = path.join(current, parts[i])
      try {
        const stat = fs.lstatSync(current)
        if (stat.isSymbolicLink()) {
          return false
        }
        const isLeaf = i === parts.length - 1
        if (isLeaf && stat.isFile() && stat.nlink > 1) {
          return false
        }
      } catch (err: unknown) {
        if ((err as { code?: string })?.code === 'ENOENT') {
          break
        }
        throw err
      }
    }
  }
  return true
}

export async function storeStatus (maybeOpts: StoreStatusOptions): Promise<string[]> {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendStoreStatusOptions(maybeOpts)
  const {
    storeDir,
    skipped,
    virtualStoreDir,
    wantedLockfile,
  } = await getContextForSingleImporter({}, {
    ...opts,
    extraBinPaths: [], // ctx.extraBinPaths is not needed, so this is fine
  })
  if (!wantedLockfile) return []

  const pkgs = (Object.entries(wantedLockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>)
    .filter(([depPath]) => !skipped.has(depPath))
    .map(([depPath, pkgSnapshot]) => {
      const id = packageIdFromSnapshot(depPath, pkgSnapshot)
      const resolution = pkgSnapshot.resolution as TarballResolution
      return {
        depPath,
        id,
        resolution,
        pkgPath: depPath,
        ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
      }
    })

  const storeIndex = new StoreIndex(storeDir)
  try {
    const modified = await pFilter(pkgs, async ({ id, resolution, depPath, name }) => {
      const pkgIndexFilePath = pickStoreIndexKey(resolution, id, { built: true })
      const pkgFilesIndex = storeIndex.get(pkgIndexFilePath) as PackageFilesIndex | undefined
      if (!pkgFilesIndex) {
        return false
      }
      const targetDir = path.join(virtualStoreDir, dp.depPathToFilename(depPath, maybeOpts.virtualStoreDirMaxLength), 'node_modules', name)
      if (!fs.existsSync(targetDir)) {
        return true
      }
      const { algo, files } = pkgFilesIndex
      const fileEntries = getMapEntries<PackageFileInfo>(files)
      // Transform files to dint format: { integrity: '<algo>-<base64>', size: number }
      const dintFiles: Record<string, { integrity: string, size: number }> = {}
      for (const [filePath, { digest, size }] of fileEntries) {
        dintFiles[filePath] = {
          integrity: formatIntegrity(algo, digest),
          size,
        }
      }
      if (await dint.check(targetDir, dintFiles)) {
        return false
      }
      const sideEffectsDiffs = getSideEffectsDiffs(pkgFilesIndex.sideEffects)
      if (sideEffectsDiffs.length > 0) {
        const sideEffectsChecks = await Promise.all(
          sideEffectsDiffs.map(async (diff) => {
            const sideEffectsDintFiles: Record<string, { integrity: string, size: number }> = {}
            const deleted = new Set(diff.deleted ?? [])
            for (const [filePath, { digest, size }] of fileEntries) {
              if (!deleted.has(filePath)) {
                sideEffectsDintFiles[filePath] = {
                  integrity: formatIntegrity(algo, digest),
                  size,
                }
              }
            }
            const addedEntries = getMapEntries<PackageFileInfo>(diff.added)
            for (const [filePath, { digest, size }] of addedEntries) {
              sideEffectsDintFiles[filePath] = {
                integrity: formatIntegrity(algo, digest),
                size,
              }
            }
            return dint.check(targetDir, sideEffectsDintFiles)
          })
        )
        if (sideEffectsChecks.some(Boolean)) {
          return false
        }
      }
      const requiresBuild =
        pkgFilesIndex.requiresBuild === true ||
        pkgRequiresBuild(pkgFilesIndex.manifest, pkgFilesIndex.files)
      if (requiresBuild && isIsolatedDir(targetDir, fileEntries.map(([filePath]) => filePath))) {
        return false
      }
      return true
    }, { concurrency: 8 })

    if ((reporter != null) && typeof reporter === 'function') {
      streamParser.removeListener('data', reporter)
    }

    return modified.map(({ pkgPath }) => pkgPath)
  } finally {
    storeIndex.close()
  }
}
