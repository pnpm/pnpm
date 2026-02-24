import crypto from 'crypto'
import fs from 'fs'
import util from 'util'
import { PnpmError } from '@pnpm/error'
import { type PackageFiles, type PackageFileInfo, type SideEffects, type FilesMap } from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { type BundledManifest } from '@pnpm/types'
import rimraf from '@zkochan/rimraf'
import { getFilePathByModeInCafs } from './getFilePathInCafs.js'

export interface Integrity {
  digest: string
  algorithm: string
}

// We track how many files were checked during installation.
// It should be rare that a files content should be checked.
// If it happens too frequently, something is wrong.
// Checking a file's integrity is an expensive operation!
// @ts-expect-error
global['verifiedFileIntegrity'] = 0

export interface VerifyResult {
  passed: boolean
  filesMap: FilesMap
  sideEffectsMaps?: Map<string, { added?: FilesMap, deleted?: string[] }>
}

export interface PackageFilesIndex {
  manifest?: BundledManifest
  requiresBuild?: boolean
  algo: string
  files: PackageFiles
  sideEffects?: SideEffects
}

export function checkPkgFilesIntegrity (
  storeDir: string,
  pkgIndex: PackageFilesIndex
): VerifyResult {
  // It might make sense to use this cache for all files in the store
  // but there's a smaller chance that the same file will be checked twice
  // so it's probably not worth the memory (this assumption should be verified)
  const verifiedFilesCache = new Set<string>()
  const _checkFilesIntegrity = checkFilesIntegrity.bind(null, verifiedFilesCache, storeDir, pkgIndex.algo)
  const verified = _checkFilesIntegrity(pkgIndex.files)
  if (!verified.passed) return verified

  const sideEffectsMaps = new Map<string, { added?: FilesMap, deleted?: string[] }>()
  if (pkgIndex.sideEffects) {
    // We verify all side effects cache. We could optimize it to verify only the side effects cache
    // that satisfies the current os/arch/platform.
    // However, it likely won't make a big difference.
    for (const [sideEffectName, { added, deleted }] of pkgIndex.sideEffects) {
      if (added) {
        const result = _checkFilesIntegrity(added)
        if (!result.passed) {
          // Skip invalid side effects
          continue
        } else {
          sideEffectsMaps.set(sideEffectName, { added: result.filesMap, deleted })
        }
      } else if (deleted) {
        sideEffectsMaps.set(sideEffectName, { deleted })
      }
    }
  }

  return {
    ...verified,
    sideEffectsMaps: sideEffectsMaps.size > 0 ? sideEffectsMaps : undefined,
  }
}

/**
 * Builds file maps from package index without verification.
 * This is a lightweight alternative to checkPkgFilesIntegrity when verifyStoreIntegrity is disabled.
 */
export function buildFileMapsFromIndex (
  storeDir: string,
  pkgIndex: PackageFilesIndex
): VerifyResult {
  const filesMap: FilesMap = new Map()

  for (const [f, fstat] of pkgIndex.files) {
    const filename = getFilePathByModeInCafs(storeDir, fstat.digest, fstat.mode)
    filesMap.set(f, filename)
  }

  const sideEffectsMaps = new Map<string, { added?: FilesMap, deleted?: string[] }>()
  if (pkgIndex.sideEffects) {
    for (const [sideEffectName, { added, deleted }] of pkgIndex.sideEffects) {
      const sideEffectEntry: { added?: FilesMap, deleted?: string[] } = {}

      if (added) {
        const addedFilesMap: FilesMap = new Map()
        for (const [f, fstat] of added) {
          const filename = getFilePathByModeInCafs(storeDir, fstat.digest, fstat.mode)
          addedFilesMap.set(f, filename)
        }
        sideEffectEntry.added = addedFilesMap
      }

      if (deleted) {
        sideEffectEntry.deleted = deleted
      }

      sideEffectsMaps.set(sideEffectName, sideEffectEntry)
    }
  }

  return {
    passed: true,
    filesMap,
    sideEffectsMaps: sideEffectsMaps.size > 0 ? sideEffectsMaps : undefined,
  }
}

function checkFilesIntegrity (
  verifiedFilesCache: Set<string>,
  storeDir: string,
  algo: string,
  files: PackageFiles
): VerifyResult {
  let allVerified = true
  const filesMap: FilesMap = new Map()

  for (const [f, fstat] of files) {
    if (!fstat.digest) {
      throw new PnpmError('MISSING_CONTENT_DIGEST', `Content digest is missing for ${f}`)
    }
    const filename = getFilePathByModeInCafs(storeDir, fstat.digest, fstat.mode)
    filesMap.set(f, filename)

    if (verifiedFilesCache.has(filename)) continue
    const passed = verifyFile(filename, fstat, algo)
    if (passed) {
      verifiedFilesCache.add(filename)
    } else {
      allVerified = false
    }
  }
  return {
    passed: allVerified,
    filesMap,
  }
}

type FileInfo = Pick<PackageFileInfo, 'size' | 'checkedAt' | 'digest'>

function verifyFile (
  filename: string,
  fstat: FileInfo,
  algorithm: string
): boolean {
  const currentFile = checkFile(filename, fstat.checkedAt)
  if (currentFile == null) return false
  if (currentFile.isModified) {
    if (currentFile.size !== fstat.size) {
      rimraf.sync(filename)
      return false
    }
    return verifyFileIntegrity(filename, { digest: fstat.digest, algorithm })
  }
  // If a file was not edited, we are skipping integrity check.
  // We assume that nobody will manually remove a file in the store and create a new one.
  return true
}

export function verifyFileIntegrity (
  filename: string,
  integrity: Integrity
): boolean {
  // @ts-expect-error
  global['verifiedFileIntegrity']++
  let data: Buffer
  try {
    data = gfs.readFileSync(filename)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return false
    }
    throw err
  }
  let computedDigest: string
  try {
    computedDigest = crypto.hash(integrity.algorithm, data, 'hex')
  } catch {
    // Invalid algorithm (e.g., corrupted index file) - treat as verification failure
    return false
  }
  const passed = computedDigest === integrity.digest
  if (!passed) {
    gfs.unlinkSync(filename)
  }
  return passed
}

function checkFile (filename: string, checkedAt?: number): { isModified: boolean, size: number } | null {
  try {
    const { mtimeMs, size } = fs.statSync(filename)
    return {
      isModified: (mtimeMs - (checkedAt ?? 0)) > 100,
      size,
    }
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return null
    throw err
  }
}
