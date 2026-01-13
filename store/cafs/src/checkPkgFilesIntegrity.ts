import fs from 'fs'
import util from 'util'
import { type PackageFilesRaw, type PackageFileInfo, type SideEffectsRaw, type FilesMap } from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { type DependencyManifest } from '@pnpm/types'
import rimraf from '@zkochan/rimraf'
import ssri from 'ssri'
import { getFilePathByModeInCafs } from './getFilePathInCafs.js'
import { parseJsonBufferSync } from './parseJson.js'
import { readManifestFromStore } from './readManifestFromStore.js'

// We track how many files were checked during installation.
// It should be rare that a files content should be checked.
// If it happens too frequently, something is wrong.
// Checking a file's integrity is an expensive operation!
// @ts-expect-error
global['verifiedFileIntegrity'] = 0

export interface VerifyResult {
  passed: boolean
  manifest?: DependencyManifest
  filesMap: FilesMap
  sideEffectsMaps?: Map<string, { added?: FilesMap, deleted?: string[] }>
}

export interface PackageFilesIndex {
  // name and version are nullable for backward compatibility
  // the initial specs of pnpm store v3 did not require these fields.
  // However, it might be possible that some types of dependencies don't
  // have the name/version fields, like the local tarball dependencies.
  name?: string
  version?: string
  requiresBuild?: boolean

  files: PackageFilesRaw
  sideEffects?: SideEffectsRaw
}

export function checkPkgFilesIntegrity (
  storeDir: string,
  pkgIndex: PackageFilesIndex,
  readManifest?: boolean
): VerifyResult {
  // It might make sense to use this cache for all files in the store
  // but there's a smaller chance that the same file will be checked twice
  // so it's probably not worth the memory (this assumption should be verified)
  const verifiedFilesCache = new Set<string>()
  const _checkFilesIntegrity = checkFilesIntegrity.bind(null, verifiedFilesCache, storeDir)
  const verified = _checkFilesIntegrity(pkgIndex.files, readManifest)
  if (!verified.passed) return verified

  const sideEffectsMaps = new Map<string, { added?: FilesMap, deleted?: string[] }>()
  if (pkgIndex.sideEffects) {
    // We verify all side effects cache. We could optimize it to verify only the side effects cache
    // that satisfies the current os/arch/platform.
    // However, it likely won't make a big difference.
    for (const [sideEffectName, { added, deleted }] of Object.entries(pkgIndex.sideEffects)) {
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
  pkgIndex: PackageFilesIndex,
  readManifest?: boolean
): VerifyResult {
  const filesMap: FilesMap = new Map()

  for (const [f, fstat] of Object.entries(pkgIndex.files)) {
    const filename = getFilePathByModeInCafs(storeDir, fstat.integrity, fstat.mode)
    filesMap.set(f, filename)
  }

  const sideEffectsMaps = new Map<string, { added?: FilesMap, deleted?: string[] }>()
  if (pkgIndex.sideEffects) {
    for (const [sideEffectName, { added, deleted }] of Object.entries(pkgIndex.sideEffects)) {
      const sideEffectEntry: { added?: FilesMap, deleted?: string[] } = {}

      if (added) {
        const addedFilesMap: FilesMap = new Map()
        for (const [f, fstat] of Object.entries(added)) {
          const filename = getFilePathByModeInCafs(storeDir, fstat.integrity, fstat.mode)
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
    manifest: readManifest ? readManifestFromStore(storeDir, pkgIndex) : undefined,
    filesMap,
    sideEffectsMaps: sideEffectsMaps.size > 0 ? sideEffectsMaps : undefined,
  }
}

function checkFilesIntegrity (
  verifiedFilesCache: Set<string>,
  storeDir: string,
  files: PackageFilesRaw,
  readManifest?: boolean
): VerifyResult {
  let allVerified = true
  let manifest: DependencyManifest | undefined
  const filesMap: FilesMap = new Map()

  for (const [f, fstat] of Object.entries(files)) {
    if (!fstat.integrity) {
      throw new Error(`Integrity checksum is missing for ${f}`)
    }
    const filename = getFilePathByModeInCafs(storeDir, fstat.integrity, fstat.mode)
    filesMap.set(f, filename)

    const readFile = readManifest && f === 'package.json'
    if (!readFile && verifiedFilesCache.has(filename)) continue
    const verifyResult = verifyFile(filename, fstat, readFile)
    if (readFile) {
      manifest = verifyResult.manifest
    }
    if (verifyResult.passed) {
      verifiedFilesCache.add(filename)
    } else {
      allVerified = false
    }
  }
  return {
    passed: allVerified,
    manifest,
    filesMap,
  }
}

type FileInfo = Pick<PackageFileInfo, 'size' | 'checkedAt'> & {
  integrity: string | ssri.IntegrityLike
}

function verifyFile (
  filename: string,
  fstat: FileInfo,
  readManifest?: boolean
): Pick<VerifyResult, 'passed' | 'manifest'> {
  const currentFile = checkFile(filename, fstat.checkedAt)
  if (currentFile == null) return { passed: false }
  if (currentFile.isModified) {
    if (currentFile.size !== fstat.size) {
      rimraf.sync(filename)
      return { passed: false }
    }
    return verifyFileIntegrity(filename, fstat, readManifest)
  }
  if (readManifest) {
    return {
      passed: true,
      manifest: parseJsonBufferSync(gfs.readFileSync(filename)) as DependencyManifest,
    }
  }
  // If a file was not edited, we are skipping integrity check.
  // We assume that nobody will manually remove a file in the store and create a new one.
  return { passed: true }
}

export function verifyFileIntegrity (
  filename: string,
  expectedFile: FileInfo,
  readManifest?: boolean
): Pick<VerifyResult, 'passed' | 'manifest'> {
  // @ts-expect-error
  global['verifiedFileIntegrity']++
  try {
    const data = gfs.readFileSync(filename)
    const passed = Boolean(ssri.checkData(data, expectedFile.integrity))
    if (!passed) {
      gfs.unlinkSync(filename)
      return { passed }
    } else if (readManifest) {
      return {
        passed,
        manifest: parseJsonBufferSync(data) as DependencyManifest,
      }
    }
    return { passed }
  } catch (err: unknown) {
    switch (util.types.isNativeError(err) && 'code' in err && err.code) {
    case 'ENOENT': return { passed: false }
    case 'EINTEGRITY': {
      // Broken files are removed from the store
      gfs.unlinkSync(filename)
      return { passed: false }
    }
    }
    throw err
  }
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
