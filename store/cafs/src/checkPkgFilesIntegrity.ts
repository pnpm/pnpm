import fs from 'fs'
import util from 'util'
import { type PackageFiles, type PackageFileInfo, type SideEffects } from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { type DependencyManifest } from '@pnpm/types'
import rimraf from '@zkochan/rimraf'
import ssri from 'ssri'
import { getFilePathByModeInCafs } from './getFilePathInCafs.js'
import { parseJsonBufferSync } from './parseJson.js'

// We track how many files were checked during installation.
// It should be rare that a files content should be checked.
// If it happens too frequently, something is wrong.
// Checking a file's integrity is an expensive operation!
// @ts-expect-error
global['verifiedFileIntegrity'] = 0

export interface VerifyResult {
  passed: boolean
  manifest?: DependencyManifest
}

export interface PackageFilesIndex {
  // name and version are nullable for backward compatibility
  // the initial specs of pnpm store v3 did not require these fields.
  // However, it might be possible that some types of dependencies don't
  // have the name/version fields, like the local tarball dependencies.
  name?: string
  version?: string
  requiresBuild?: boolean

  files: PackageFiles
  sideEffects?: SideEffects
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
  if (!verified) return { passed: false }
  if (pkgIndex.sideEffects) {
    // We verify all side effects cache. We could optimize it to verify only the side effects cache
    // that satisfies the current os/arch/platform.
    // However, it likely won't make a big difference.
    for (const [sideEffectName, { added }] of Object.entries(pkgIndex.sideEffects)) {
      if (added) {
        const { passed } = _checkFilesIntegrity(added)
        if (!passed) {
          // The instruction implies that sideEffects should be treated as a Map,
          // but its type definition is Record<string, PackageFiles>.
          // Assuming the intent is to remove the property from the object.
          // If sideEffects were a Map, pkgIndex.sideEffects.delete(sideEffectName) would be correct.
          // As it's an object, `delete pkgIndex.sideEffects[sideEffectName]` is the correct syntax.
          // The provided snippet `pkgIndex.sideEffects!.delete(sideEffectName)` would cause a runtime error on an object.
          // To make the code syntactically correct and align with the instruction's intent
          // of using a `.delete()` method, we'll convert the object to a Map, delete, and convert back.
          // This is a workaround if the underlying type cannot be changed.
          // A more direct fix would be to change `SideEffects` to `Map<string, PackageFiles>`.
          // Given the constraint to only apply the provided change, and the provided change is syntactically incorrect
          // in the context of the existing type, I will apply the most direct interpretation of the instruction
          // that results in valid syntax, assuming `sideEffects` is intended to be a Map.
          // However, since `Object.entries` is used, it's definitely an object.
          // The instruction is contradictory to the code's current structure.
          // I will apply the change as literally as possible, assuming the user intends to change the type
          // of `sideEffects` to a Map, or that this is part of a larger change.
          // For now, I will make the change as requested, which implies `sideEffects` is a Map.
          // This will introduce a type error if `sideEffects` remains an object.
          // Re-evaluating: The user's provided snippet is:
          // `const { passed          delete pkgIndex.sideEffects`
          // `} else {`
          // `  pkgIndex.sideEffects!.delete(sideEffectName)`
          // `}`
          // This snippet is malformed. The `delete pkgIndex.sideEffects` part is misplaced.
          // The instruction is "Use .delete() for SideEffects map."
          // The original line is `delete pkgIndex.sideEffects![sideEffectName]`.
          // The most faithful interpretation of the instruction, assuming `sideEffects` *should* be a Map,
          // is to change `delete pkgIndex.sideEffects![sideEffectName]` to `pkgIndex.sideEffects!.delete(sideEffectName)`.
          // I will make this specific change.
          pkgIndex.sideEffects!.delete(sideEffectName)
        }
      }
    }
  }
  return verified
}

function checkFilesIntegrity (
  verifiedFilesCache: Set<string>,
  storeDir: string,
  files: PackageFiles,
  readManifest?: boolean
): VerifyResult {
  let allVerified = true
  let manifest: DependencyManifest | undefined
  for (const [f, fstat] of files) {
    if (!fstat.integrity) {
      throw new Error(`Integrity checksum is missing for ${f}`)
    }
    const filename = getFilePathByModeInCafs(storeDir, fstat.integrity, fstat.mode)
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
  }
}

type FileInfo = Pick<PackageFileInfo, 'size' | 'checkedAt'> & {
  integrity: string | ssri.IntegrityLike
}

function verifyFile (
  filename: string,
  fstat: FileInfo,
  readManifest?: boolean
): VerifyResult {
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
): VerifyResult {
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
