import crypto from 'node:crypto'
import fs from 'node:fs'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import gfs from '@pnpm/fs.graceful-fs'
import type { FilesMap, PackageFileInfo, PackageFiles, SideEffects } from '@pnpm/store.cafs-types'
import type { BundledManifest } from '@pnpm/types'
import { rimrafSync } from '@zkochan/rimraf'

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

export async function checkPkgFilesIntegrity (
  storeDir: string,
  pkgIndex: PackageFilesIndex
): Promise<VerifyResult> {
  const verifiedFilesCache = new Set<string>()
  const _checkFilesIntegrity = checkFilesIntegrity.bind(null, verifiedFilesCache, storeDir, pkgIndex.algo)
  const verified = await _checkFilesIntegrity(pkgIndex.files)
  if (!verified.passed) return verified

  const sideEffectsMaps = new Map<string, { added?: FilesMap, deleted?: string[] }>()
  if (pkgIndex.sideEffects) {
    for (const [sideEffectName, { added, deleted }] of pkgIndex.sideEffects) {
      if (added) {
        const result = await _checkFilesIntegrity(added)
        if (!result.passed) {
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

async function checkFilesIntegrity (
  verifiedFilesCache: Set<string>,
  storeDir: string,
  algo: string,
  files: PackageFiles
): Promise<VerifyResult> {
  let allVerified = true
  const filesMap: FilesMap = new Map()

  const verifyPromises: Promise<void>[] = []

  for (const [f, fstat] of files) {
    if (!fstat.digest) {
      throw new PnpmError('MISSING_CONTENT_DIGEST', `Content digest is missing for ${f}`)
    }
    const filename = getFilePathByModeInCafs(storeDir, fstat.digest, fstat.mode)
    filesMap.set(f, filename)

    if (verifiedFilesCache.has(filename)) continue

    verifyPromises.push(
      verifyFile(filename, fstat, algo).then((passed) => {
        if (passed) {
          verifiedFilesCache.add(filename)
        } else {
          allVerified = false
        }
      })
    )
  }

  await Promise.all(verifyPromises)

  return {
    passed: allVerified,
    filesMap,
  }
}

type FileInfo = Pick<PackageFileInfo, 'size' | 'checkedAt' | 'digest'>

async function verifyFile (
  filename: string,
  fstat: FileInfo,
  algorithm: string
): Promise<boolean> {
  const currentFile = await checkFile(filename, fstat.checkedAt)
  if (currentFile == null) return false
  if (currentFile.isModified) {
    if (currentFile.size !== fstat.size) {
      rimrafSync(filename)
      return false
    }
    const passed = await verifyFileIntegrityAsync(filename, { digest: fstat.digest, algorithm })
    if (!passed) {
      gfs.unlinkSync(filename)
    }
    return passed
  }
  return true
}

export async function verifyFileIntegrityAsync (
  filename: string,
  integrity: Integrity
): Promise<boolean> {
  // @ts-expect-error
  global['verifiedFileIntegrity']++
  let hash: crypto.Hash
  try {
    hash = crypto.createHash(integrity.algorithm)
  } catch {
    // Invalid algorithm (e.g., corrupted index file) - treat as verification failure
    return false
  }
  return new Promise<boolean>((resolve, reject) => {
    const stream = fs.createReadStream(filename)
    stream.on('data', (chunk: string | Buffer) => hash.update(chunk))
    stream.on('end', () => {
      try {
        resolve(hash.digest('hex') === integrity.digest)
      } catch {
        resolve(false)
      }
    })
    stream.on('error', (err: NodeJS.ErrnoException) => {
      if (err.code === 'ENOENT') {
        resolve(false)
      } else {
        reject(err)
      }
    })
  })
}

async function checkFile (filename: string, checkedAt?: number): Promise<{ isModified: boolean, size: number } | null> {
  try {
    const { mtimeMs, size } = await fs.promises.stat(filename)
    return {
      isModified: (mtimeMs - (checkedAt ?? 0)) > 100,
      size,
    }
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return null
    throw err
  }
}
