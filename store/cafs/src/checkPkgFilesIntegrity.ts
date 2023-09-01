import fs from 'fs'
import type { DeferredManifestPromise, PackageFileInfo } from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import rimraf from '@zkochan/rimraf'
import ssri from 'ssri'
import { getFilePathByModeInCafs } from './getFilePathInCafs'
import { parseJsonBuffer } from './parseJson'

// We track how many files were checked during installation.
// It should be rare that a files content should be checked.
// If it happens too frequently, something is wrong.
// Checking a file's integrity is an expensive operation!
// @ts-expect-error
global['verifiedFileIntegrity'] = 0

export interface PackageFilesIndex {
  // name and version are nullable for backward compatibility
  // the initial specs of pnpm store v3 did not require these fields.
  // However, it might be possible that some types of dependencies don't
  // have the name/version fields, like the local tarball dependencies.
  name?: string
  version?: string

  files: Record<string, PackageFileInfo>
  sideEffects?: Record<string, Record<string, PackageFileInfo>>
}

export function checkPkgFilesIntegrity (
  cafsDir: string,
  pkgIndex: PackageFilesIndex,
  manifest?: DeferredManifestPromise
) {
  // It might make sense to use this cache for all files in the store
  // but there's a smaller chance that the same file will be checked twice
  // so it's probably not worth the memory (this assumption should be verified)
  const verifiedFilesCache = new Set<string>()
  const _checkFilesIntegrity = checkFilesIntegrity.bind(null, verifiedFilesCache, cafsDir)
  const verified = _checkFilesIntegrity(pkgIndex.files, manifest)
  if (!verified) return false
  if (pkgIndex.sideEffects) {
    // We verify all side effects cache. We could optimize it to verify only the side effects cache
    // that satisfies the current os/arch/platform.
    // However, it likely won't make a big difference.
    for (const [sideEffectName, files] of Object.entries(pkgIndex.sideEffects)) {
      if (!_checkFilesIntegrity(files)) {
        delete pkgIndex.sideEffects![sideEffectName]
      }
    }
  }
  return true
}

function checkFilesIntegrity (
  verifiedFilesCache: Set<string>,
  cafsDir: string,
  files: Record<string, PackageFileInfo>,
  manifest?: DeferredManifestPromise
): boolean {
  let allVerified = true
  for (const [f, fstat] of Object.entries(files)) {
    if (!fstat.integrity) {
      throw new Error(`Integrity checksum is missing for ${f}`)
    }
    const filename = getFilePathByModeInCafs(cafsDir, fstat.integrity, fstat.mode)
    const deferredManifest = manifest && f === 'package.json' ? manifest : undefined
    if (!deferredManifest && verifiedFilesCache.has(filename)) continue
    if (verifyFile(filename, fstat, deferredManifest)) {
      verifiedFilesCache.add(filename)
    } else {
      allVerified = false
    }
  }
  if (manifest && !files['package.json']) {
    manifest.resolve(undefined)
  }
  return allVerified
}

type FileInfo = Pick<PackageFileInfo, 'size' | 'checkedAt'> & {
  integrity: string | ssri.IntegrityLike
}

function verifyFile (
  filename: string,
  fstat: FileInfo,
  deferredManifest?: DeferredManifestPromise
): boolean {
  const currentFile = checkFile(filename, fstat.checkedAt)
  if (currentFile == null) return false
  if (currentFile.isModified) {
    if (currentFile.size !== fstat.size) {
      rimraf.sync(filename)
      return false
    }
    return verifyFileIntegrity(filename, fstat, deferredManifest)
  }
  if (deferredManifest != null) {
    parseJsonBuffer(gfs.readFileSync(filename), deferredManifest)
  }
  // If a file was not edited, we are skipping integrity check.
  // We assume that nobody will manually remove a file in the store and create a new one.
  return true
}

export function verifyFileIntegrity (
  filename: string,
  expectedFile: FileInfo,
  deferredManifest?: DeferredManifestPromise
): boolean {
  // @ts-expect-error
  global['verifiedFileIntegrity']++
  try {
    const data = gfs.readFileSync(filename)
    const ok = Boolean(ssri.checkData(data, expectedFile.integrity))
    if (!ok) {
      gfs.unlinkSync(filename)
    } else if (deferredManifest != null) {
      parseJsonBuffer(data, deferredManifest)
    }
    return ok
  } catch (err: any) { // eslint-disable-line
    switch (err.code) {
    case 'ENOENT': return false
    case 'EINTEGRITY': {
      // Broken files are removed from the store
      gfs.unlinkSync(filename)
      return false
    }
    }
    throw err
  }
}

function checkFile (filename: string, checkedAt?: number) {
  try {
    const { mtimeMs, size } = fs.statSync(filename)
    return {
      isModified: (mtimeMs - (checkedAt ?? 0)) > 100,
      size,
    }
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') return null
    throw err
  }
}
