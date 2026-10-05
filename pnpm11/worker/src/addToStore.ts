import { createHash } from 'node:crypto'
import { createReadStream } from 'node:fs'

import { pkgRequiresBuild } from '@pnpm/building.pkg-requires-build'
import { formatIntegrity, parseIntegrity } from '@pnpm/crypto.integrity'
import { isError, PnpmError } from '@pnpm/error'
import {
  type CafsFunctions,
  type FilesIndex,
  HASH_ALGORITHM,
  normalizeBundledManifest,
  type PackageFilesIndex,
} from '@pnpm/store.cafs'
import type { AddToStoreResult, FilesMap, PackageFiles, SideEffectsDiff } from '@pnpm/store.cafs-types'
import { packForStorage, storeIndexKey } from '@pnpm/store.index'
import type { BundledManifest, DependencyManifest } from '@pnpm/types'

import { hashBuffer } from './hashBuffer.js'
import { getCafs, getStoreIndex } from './storeCaches.js'
import type { AddDirToStoreMessage, TarballExtractMessage } from './types.js'

interface IndexWrite {
  key: string
  buffer: Uint8Array
}

interface AddFilesFromDirResult {
  status: string
  value: {
    filesMap: FilesMap
    manifest?: BundledManifest
    requiresBuild: boolean
    requiresPrepare?: boolean
    sideEffects?: SideEffectsDiff
  }
  indexWrites?: IndexWrite[]
}

interface AddedPkgFiles {
  filesIntegrity: PackageFiles
  filesMap: FilesMap
  manifest?: DependencyManifest
  bundledManifest?: BundledManifest
}

interface AddTarballToStoreResult {
  status: string
  value: {
    filesMap: FilesMap
    manifest?: BundledManifest
    requiresBuild: boolean
    integrity: string
  }
  indexWrites: IndexWrite[]
}

interface TarballIntegrityFailure {
  status: string
  error: {
    type: 'integrity_validation_failed'
    algorithm: string
    expected: string
    found: string
  }
}

export async function addTarballToStore (
  { buffer, tarballFile, storeDir, integrity, filesIndexFile, pkgId, appendManifest, ignoreFilePattern }: TarballExtractMessage
): Promise<AddTarballToStoreResult | TarballIntegrityFailure> {
  if (buffer == null && tarballFile == null) throw new Error('Tarball extraction has no source')
  const calculatedIntegrity = tarballFile != null ? await hashTarballFile(tarballFile, integrity) : undefined
  const integrityFailure = checkIntegrity({ buffer, integrity, calculatedIntegrity })
  if (integrityFailure) return integrityFailure
  const cafs = getCafs(storeDir)
  const ignore = ignoreFilePattern ? makeIgnoreFromPattern(ignoreFilePattern) : undefined
  const addedFiles = tarballFile != null
    ? await cafs.addFilesFromTarballFile(tarballFile, true, ignore)
    : await cafs.addFilesFromTarballBounded(buffer!, true, ignore)
  const added = describeAddedPkgFiles(cafs, addedFiles, appendManifest)
  const requiresBuild = pkgRequiresBuild(added.bundledManifest, added.filesIntegrity)
  const pkgFilesIndex: PackageFilesIndex = {
    requiresBuild,
    manifest: added.bundledManifest,
    algo: HASH_ALGORITHM,
    files: added.filesIntegrity,
  }
  const packedFilesIndex = packToShared(pkgFilesIndex)
  const indexWrites: IndexWrite[] = [{ key: filesIndexFile, buffer: packedFilesIndex }]
  if (!integrity) {
    integrity = calculatedIntegrity ?? calcIntegrity(buffer!)
    if (pkgId) {
      indexWrites.push({ key: storeIndexKey(integrity, pkgId), buffer: packedFilesIndex })
    }
  }
  return {
    status: 'success',
    value: {
      filesMap: added.filesMap,
      manifest: added.bundledManifest,
      requiresBuild,
      integrity,
    },
    indexWrites,
  }
}

function checkIntegrity (opts: { buffer?: Buffer, integrity?: string, calculatedIntegrity?: string }): TarballIntegrityFailure | undefined {
  if (!opts.integrity) return undefined
  return opts.calculatedIntegrity != null
    ? compareTarballIntegrity(opts.calculatedIntegrity, opts.integrity)
    : checkTarballIntegrity(opts.buffer!, opts.integrity)
}

async function hashTarballFile (tarballFile: string, integrity?: string): Promise<string> {
  const algorithm = integrity ? parseIntegrity(integrity).algorithm : 'sha512'
  const hash = createHash(algorithm)
  for await (const chunk of createReadStream(tarballFile)) hash.update(chunk)
  return formatIntegrity(algorithm, hash.digest('hex'))
}

function checkTarballIntegrity (buffer: Buffer, integrity: string): TarballIntegrityFailure | undefined {
  const { algorithm } = parseIntegrity(integrity)
  return compareTarballIntegrity(formatIntegrity(algorithm, hashBuffer(algorithm, buffer)), integrity)
}

function compareTarballIntegrity (found: string, expected: string): TarballIntegrityFailure | undefined {
  const { algorithm, hexDigest } = parseIntegrity(expected)
  if (parseIntegrity(found).hexDigest === hexDigest) return undefined
  return {
    status: 'error',
    error: {
      type: 'integrity_validation_failed',
      algorithm,
      expected,
      found,
    },
  }
}

function calcIntegrity (buffer: Buffer): string {
  const calculatedHash = hashBuffer('sha512', buffer)
  return formatIntegrity('sha512', calculatedHash)
}

function makeIgnoreFromPattern (pattern: string): (filename: string) => boolean {
  // `ignoreFilePattern` is a public field on FetchOptions, so callers that don't go
  // through the binary-fetcher's validated `archiveFilters` path could still supply a
  // bad regex. Convert the SyntaxError into a PnpmError with a stable code so it's
  // actionable for users.
  let regex: RegExp
  try {
    regex = new RegExp(pattern)
  } catch (err: unknown) {
    const detail = isError(err) ? `: ${err.message}` : ''
    throw new PnpmError(
      'INVALID_IGNORE_FILE_PATTERN',
      `Invalid ignoreFilePattern regex${detail}: ${pattern}`
    )
  }
  return (filename) => regex.test(filename)
}

function packToShared (data: unknown): Uint8Array {
  const packed = packForStorage(data)
  const shared = new SharedArrayBuffer(packed.byteLength)
  const view = new Uint8Array(shared)
  view.set(packed)
  return view
}

export function addFilesFromDir (message: AddDirToStoreMessage): AddFilesFromDirResult {
  const { dir, files, includeNodeModules, sideEffectsCacheKey, storeDir } = message
  const cafs = getCafs(storeDir)
  const addResult = cafs.addFilesFromDir(dir, {
    files,
    includeNodeModules,
    readManifest: true,
    // A side-effects entry never reaches Windows with a symlink in it:
    // creating one there needs a privilege most users lack.
    recordSymlinks: sideEffectsCacheKey != null && process.platform !== 'win32',
  })
  const added = describeAddedPkgFiles(cafs, addResult, message.appendManifest)
  if (sideEffectsCacheKey) {
    return addSideEffectsToFilesIndex(message, sideEffectsCacheKey, { ...added, hasUnrecordedSymlinks: addResult.hasUnrecordedSymlinks })
  }
  const requiresBuild = pkgRequiresBuild(added.bundledManifest, added.filesIntegrity)
  const pkgFilesIndex: PackageFilesIndex = {
    requiresBuild,
    requiresPrepare: message.requiresPrepare,
    manifest: added.bundledManifest,
    algo: HASH_ALGORITHM,
    files: added.filesIntegrity,
  }
  return {
    status: 'success',
    value: {
      filesMap: added.filesMap,
      manifest: added.bundledManifest,
      requiresBuild,
      requiresPrepare: message.requiresPrepare,
      sideEffects: undefined,
    },
    indexWrites: [{ key: message.filesIndexFile, buffer: packToShared(pkgFilesIndex) }],
  }
}

function addSideEffectsToFilesIndex (
  { filesIndexFile, storeDir }: AddDirToStoreMessage,
  sideEffectsCacheKey: string,
  added: AddedPkgFiles & { hasUnrecordedSymlinks?: boolean }
): AddFilesFromDirResult {
  const { filesMap, manifest, bundledManifest } = added
  const existingFilesIndex = getStoreIndex(storeDir).get(filesIndexFile) as PackageFilesIndex | undefined
  if (!existingFilesIndex) {
    // If there is no existing index file, then we cannot store the side effects.
    return {
      status: 'success',
      value: {
        filesMap,
        manifest: bundledManifest,
        requiresBuild: pkgRequiresBuild(manifest, filesMap),
      },
    }
  }
  if (added.hasUnrecordedSymlinks) {
    return {
      status: 'success',
      value: {
        filesMap,
        manifest: bundledManifest,
        requiresBuild: existingFilesIndex.requiresBuild ?? pkgRequiresBuild(manifest, filesMap),
      },
      indexWrites: removeSideEffectsEntry(existingFilesIndex, filesIndexFile, sideEffectsCacheKey),
    }
  }
  const sideEffects = recordSideEffects(existingFilesIndex, sideEffectsCacheKey, added.filesIntegrity)
  return {
    status: 'success',
    value: {
      filesMap,
      manifest: bundledManifest,
      requiresBuild: existingFilesIndex.requiresBuild ?? pkgRequiresBuild(manifest, filesMap),
      requiresPrepare: existingFilesIndex.requiresPrepare,
      sideEffects,
    },
    indexWrites: [{ key: filesIndexFile, buffer: packToShared(existingFilesIndex) }],
  }
}

function removeSideEffectsEntry (
  existingFilesIndex: PackageFilesIndex,
  filesIndexFile: string,
  sideEffectsCacheKey: string
): IndexWrite[] | undefined {
  if (!existingFilesIndex.sideEffects?.delete(sideEffectsCacheKey)) return undefined
  if (existingFilesIndex.sideEffects.size === 0) {
    existingFilesIndex.sideEffects = undefined
  }
  return [{ key: filesIndexFile, buffer: packToShared(existingFilesIndex) }]
}

function recordSideEffects (
  existingFilesIndex: PackageFilesIndex,
  sideEffectsCacheKey: string,
  filesIntegrity: PackageFiles
): SideEffectsDiff {
  if (!existingFilesIndex.sideEffects) {
    existingFilesIndex.sideEffects = new Map()
  }
  if (existingFilesIndex.algo !== HASH_ALGORITHM) {
    throw new PnpmError(
      'ALGO_MISMATCH',
      `Algorithm mismatch: package index uses "${existingFilesIndex.algo}" but side effects were computed with "${HASH_ALGORITHM}"`
    )
  }
  const sideEffects = calculateDiff(existingFilesIndex.files, filesIntegrity)
  existingFilesIndex.sideEffects.set(sideEffectsCacheKey, sideEffects)
  return sideEffects
}

/**
 * Adds a `package.json` to the store for a package that lacks one (the
 * `appendManifest`, or else a placeholder) and splits the files index into
 * the integrity and path maps.
 */
function describeAddedPkgFiles (
  cafs: CafsFunctions,
  { filesIndex, manifest }: AddToStoreResult,
  appendManifest: DependencyManifest | undefined
): AddedPkgFiles {
  if (appendManifest && manifest == null) {
    manifest = appendManifest
    addManifestToCafs(cafs, filesIndex, appendManifest)
  } else if (!filesIndex.has('package.json')) {
    addPlaceholderPackageJsonToCafs(cafs, filesIndex)
  }
  const { filesIntegrity, filesMap } = processFilesIndex(filesIndex)
  const bundledManifest = manifest != null ? normalizeBundledManifest(manifest) : undefined
  return { filesIntegrity, filesMap, manifest, bundledManifest }
}

function addManifestToCafs (cafs: CafsFunctions, filesIndex: FilesIndex, manifest: DependencyManifest): void {
  const fileBuffer = Buffer.from(JSON.stringify(manifest, null, 2), 'utf8')
  const mode = 0o644
  filesIndex.set('package.json', {
    mode,
    size: fileBuffer.length,
    ...cafs.addFile(fileBuffer, mode),
  })
}

const PLACEHOLDER_PACKAGE_JSON = Buffer.from(JSON.stringify({ _pnpmPlaceholder: 'This file was generated by pnpm. The original package did not contain a package.json.' }), 'utf8')

// Packages that lack a package.json (e.g. injected packages in a Bit
// workspace) get a synthetic one so that package.json can serve as a
// universal completion marker for the indexed package importer.
// The _pnpmPlaceholder field tells the package requester to ignore it
// when reading the manifest.
function addPlaceholderPackageJsonToCafs (cafs: CafsFunctions, filesIndex: FilesIndex): void {
  const mode = 0o644
  filesIndex.set('package.json', {
    mode,
    size: PLACEHOLDER_PACKAGE_JSON.length,
    ...cafs.addFile(PLACEHOLDER_PACKAGE_JSON, mode),
  })
}

function calculateDiff (baseFiles: PackageFiles, sideEffectsFiles: PackageFiles): SideEffectsDiff {
  const deleted: string[] = []
  const added: PackageFiles = new Map()
  const allFiles = new Set([...baseFiles.keys(), ...sideEffectsFiles.keys()])
  for (const file of allFiles) {
    if (!sideEffectsFiles.has(file)) {
      deleted.push(file)
    } else if (
      !baseFiles.has(file) ||
      baseFiles.get(file)!.digest !== sideEffectsFiles.get(file)!.digest ||
      // On Windows, the mode read back from disk does not preserve the mode stored from the tarball.
      (process.platform !== 'win32' && baseFiles.get(file)!.mode !== sideEffectsFiles.get(file)!.mode)
    ) {
      added.set(file, sideEffectsFiles.get(file)!)
    }
  }
  const diff: SideEffectsDiff = {}
  if (deleted.length > 0) {
    diff.deleted = deleted
  }
  if (added.size > 0) {
    diff.added = added
  }
  return diff
}

interface ProcessFilesIndexResult {
  filesIntegrity: PackageFiles
  filesMap: FilesMap
}

function processFilesIndex (filesIndex: FilesIndex): ProcessFilesIndexResult {
  const filesIntegrity: PackageFiles = new Map()
  const filesMap: FilesMap = new Map()
  for (const [relativePath, { checkedAt, filePath, digest, mode, size }] of filesIndex) {
    filesIntegrity.set(relativePath, {
      checkedAt,
      digest,
      mode,
      size,
    })
    filesMap.set(relativePath, filePath)
  }
  return { filesIntegrity, filesMap }
}
