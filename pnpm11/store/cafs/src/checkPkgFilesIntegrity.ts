import crypto from 'node:crypto'
import fs from 'node:fs'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import gfs from '@pnpm/fs.graceful-fs'
import type { FilesMap, PackageFileInfo, PackageFiles, RemoteSideEffectsQuarantine, SideEffects } from '@pnpm/store.cafs-types'
import type { BundledManifest } from '@pnpm/types'
import { rimrafSync } from '@zkochan/rimraf'

import { getFilePathByModeInCafs } from './getFilePathInCafs.js'

const CHUNK_SIZE = 64 * 1024
// Windows has neither flag; there the descriptor check below stands alone.
// O_NONBLOCK keeps a FIFO planted at a digest path from holding the open
// until a writer appears, and is a no-op for the regular files expected.
const GUARDED_OPEN = (fs.constants.O_NOFOLLOW ?? 0) | (fs.constants.O_NONBLOCK ?? 0)

export interface Integrity {
  digest: string
  algorithm: string
}

export interface VerifiedFileIntegrity {
  files: number
  ms: number
}

/**
 * How many store files had to be re-hashed to verify them, and how long
 * that took. It should be rare for a file's content to be checked — the
 * recorded `checkedAt` usually says the file is untouched — and hashing
 * is expensive, so an install that spends a noticeable amount of time
 * here reports it.
 *
 * Verification runs in worker threads, so this is a per-worker tally.
 * Each worker hands its share back with the response it is answering
 * (see `@pnpm/worker`), and the main thread sums them up.
 */
let verifiedFileIntegrity: VerifiedFileIntegrity = { files: 0, ms: 0 }

/**
 * The tally accumulated since the last call, resetting it. The worker
 * calls this once per response so every re-hash is reported to the main
 * thread exactly once.
 */
export function takeVerifiedFileIntegrity (): VerifiedFileIntegrity {
  const taken = verifiedFileIntegrity
  verifiedFileIntegrity = { files: 0, ms: 0 }
  return taken
}

export interface VerifyResult {
  passed: boolean
  filesMap: FilesMap
  sideEffectsMaps?: Map<string, { added?: FilesMap, deleted?: string[] }>
  sideEffectsDiffs?: SideEffects
  remoteSideEffectsQuarantine?: RemoteSideEffectsQuarantine
}

export interface PackageFilesIndex {
  manifest?: BundledManifest
  requiresBuild?: boolean
  /** Whether preparing a git package required lifecycle scripts before these files were stored. */
  requiresPrepare?: boolean
  algo: string
  files: PackageFiles
  sideEffects?: SideEffects
  remoteSideEffectsQuarantine?: RemoteSideEffectsQuarantine
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
    sideEffectsDiffs: sideEffectsMaps.size > 0 ? matchingSideEffects(pkgIndex.sideEffects, sideEffectsMaps) : undefined,
    remoteSideEffectsQuarantine: pkgIndex.remoteSideEffectsQuarantine,
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
    sideEffectsDiffs: sideEffectsMaps.size > 0 ? matchingSideEffects(pkgIndex.sideEffects, sideEffectsMaps) : undefined,
    remoteSideEffectsQuarantine: pkgIndex.remoteSideEffectsQuarantine,
  }
}

function matchingSideEffects (
  sideEffects: SideEffects | undefined,
  sideEffectsMaps: Map<string, unknown>
): SideEffects | undefined {
  if (sideEffects == null) return undefined
  return new Map(Array.from(sideEffects).filter(([cacheKey]) => sideEffectsMaps.has(cacheKey)))
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
      scrubDirectoryAtCafsPath(filename)
      return false
    }
    const passed = tallyVerifyFileIntegrity(filename, { digest: fstat.digest, algorithm })
    if (!passed) {
      scrubDirectoryAtCafsPath(filename)
    }
    return passed
  }
  // Fast path for trusted stores: if metadata says the file is unchanged, skip the
  // digest read. Store integrity verification detects corruption; it does not make
  // a store writable by untrusted users safe.
  return true
}

/**
 * Removes a directory squatting at a CAFS blob path, so the re-fetch's rename
 * can land. Every other dirent stays: the re-fetch replaces a mismatched file
 * atomically (writeBufferToCafs), and unlinking here would race installs in
 * other processes importing from this very path (the pnpm/pnpm#14353 error
 * class) — verification also reports a transient read failure the same way as
 * a real mismatch.
 *
 * A check-then-delete on the live path could delete whatever a concurrent
 * process put there in between, so the dirent is renamed aside first and only
 * then inspected: a directory is removed at its scrub name, and anything
 * else — including a blob a concurrent re-fetch landed after the failed
 * verification — is renamed back where it was. Best-effort throughout; a
 * crash between the two renames leaves an inert `*.pnpm-scrub-*` entry in the
 * shard directory, which nothing ever resolves.
 */
let scrubCounter = 0
function scrubDirectoryAtCafsPath (filename: string): void {
  let stats
  try {
    stats = fs.lstatSync(filename)
  } catch {
    return
  }
  // Cheap gate only — the rename + inspect below re-decides
  // authoritatively, so a dirent swapped after this stat is still handled
  // correctly. Without the gate every mismatched *file* would pay the
  // rename round-trip and briefly vanish from its path.
  if (!stats.isDirectory()) return
  const scrubName = `${filename}.pnpm-scrub-${process.pid}-${scrubCounter++}`
  try {
    fs.renameSync(filename, scrubName)
  } catch {
    // Nothing left at the path (a concurrent scrubber won), or the rename
    // is not possible; either way the next install retries.
    return
  }
  try {
    if (fs.lstatSync(scrubName).isDirectory()) {
      rimrafSync(scrubName)
    } else {
      // The dirent changed between the failed verification and the
      // rename — a concurrent process replaced the squatter. Put the
      // newcomer back where every reader expects it.
      fs.renameSync(scrubName, filename)
    }
  } catch {
    // Best-effort; the next install retries.
  }
}

/**
 * `verifyFileIntegrity` with the work recorded in the tally the install
 * reports at the end. Only store verification goes through here; the
 * CAFS writer's own integrity check is not the store-wide problem the
 * report is about, and pacquet's writer doesn't hash at all.
 */
function tallyVerifyFileIntegrity (
  filename: string,
  integrity: Integrity
): boolean {
  const startedAt = performance.now()
  // A file that vanished under us, and an algorithm this Node can't
  // hash with, both hash nothing — so they belong in neither half of
  // the tally: the install reports the figures as time spent hashing.
  const data = readFileForIntegrity(filename)
  if (data == null) return false
  const passed = hashMatches(data, integrity)
  if (passed == null) return false
  verifiedFileIntegrity.files++
  verifiedFileIntegrity.ms += performance.now() - startedAt
  return passed
}

export function verifyFileIntegrity (
  filename: string,
  integrity: Integrity
): boolean {
  const data = readFileForIntegrity(filename)
  if (data == null) return false
  return hashMatches(data, integrity) ?? false
}

/**
 * Whether the file at `filename` hashes to `integrity`, read in 64 KiB
 * chunks off the event loop.
 *
 * The synchronous {@link verifyFileIntegrity} reads and hashes a whole blob
 * before yielding, which stalls everything else for the length of a large
 * CAS file. Prefer this wherever the caller is already asynchronous.
 *
 * `false` — the caller falls back to fetching the file — when the path cannot
 * be opened at all, when what was opened is not a regular file, when the
 * content does not hash to `integrity`, or when the runtime does not support
 * `integrity.algorithm`. A failure while reading an already-open file is
 * thrown instead, since the store handed over a file it then could not read.
 */
export async function verifyFileIntegrityAsync (
  filename: string,
  integrity: Integrity
): Promise<boolean> {
  let hasher: crypto.Hash
  try {
    hasher = crypto.createHash(integrity.algorithm)
  } catch {
    // An unusable algorithm, e.g. from a corrupted index file, is a
    // verification failure rather than an error, as in `hashMatches`.
    return false
  }
  // The store addresses its own regular files. A symlink at the digest path
  // would name bytes the store neither owns nor can keep from changing, so it
  // is refused at open where the platform can; inspecting the descriptor
  // afterwards keeps the check bound to the file that is actually read.
  let handle: fs.promises.FileHandle
  try {
    handle = await fs.promises.open(filename, fs.constants.O_RDONLY | GUARDED_OPEN)
  } catch {
    // Whatever turned the open away names something that cannot be reused, and
    // the caller's fallback is a verified fetch that reports any real fault
    // itself. A failure once the file is open is different: that one is left
    // to throw, since the store handed over a file it then could not read.
    return false
  }
  try {
    if (!(await handle.stat()).isFile()) return false
    // `autoClose: false` so the handle is closed once, below, whether or not
    // the stream got as far as ending.
    const stream = handle.createReadStream({ highWaterMark: CHUNK_SIZE, autoClose: false })
    for await (const chunk of stream) {
      hasher.update(chunk as Buffer)
    }
  } finally {
    await handle.close()
  }
  return hasher.digest('hex') === integrity.digest
}


/** The file's content, or `null` if it is no longer there. */
function readFileForIntegrity (filename: string): Buffer | null {
  try {
    return gfs.readFileSync(filename)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
}

/**
 * Whether `data` hashes to the recorded digest, or `null` when no hash
 * could be computed at all — an invalid algorithm, e.g. from a
 * corrupted index file. Callers treat `null` as a verification failure;
 * it stays distinct from `false` so nothing that never hashed is
 * reported as time spent hashing.
 */
function hashMatches (data: Buffer, integrity: Integrity): boolean | null {
  try {
    return crypto.hash(integrity.algorithm, data, 'hex') === integrity.digest
  } catch {
    return null
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
