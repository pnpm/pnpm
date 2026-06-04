import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'
import { parentPort } from 'node:worker_threads'

import { pkgRequiresBuild } from '@pnpm/building.pkg-requires-build'
import { formatIntegrity, parseIntegrity } from '@pnpm/crypto.integrity'
import { PnpmError } from '@pnpm/error'
import { hardLinkDir } from '@pnpm/fs.hard-link-dir'
import { symlinkDependencySync } from '@pnpm/fs.symlink-dependency'
import {
  buildFileMapsFromIndex,
  type CafsFunctions,
  checkPkgFilesIntegrity,
  createCafs,
  type FilesIndex,
  HASH_ALGORITHM,
  normalizeBundledManifest,
  type PackageFilesIndex,
  type VerifyResult,
} from '@pnpm/store.cafs'
import type { Cafs, FilesMap, PackageFiles, SideEffectsDiff } from '@pnpm/store.cafs-types'
import { createCafsStore } from '@pnpm/store.create-cafs-store'
import { packForStorage, StoreIndex } from '@pnpm/store.index'
import type { BundledManifest, DependencyManifest } from '@pnpm/types'

import { equalOrSemverEqual } from './equalOrSemverEqual.js'
import type {
  AddDirToStoreMessage,
  HardLinkDirMessage,
  InitStoreMessage,
  LinkPkgMessage,
  ReadPkgFromCafsMessage,
  SymlinkAllModulesMessage,
  TarballExtractMessage,
  WriteCafsFilesMessage,
} from './types.js'

export function startWorker (): void {
  process.on('uncaughtException', (err) => {
    console.error(err)
  })
  parentPort!.on('message', handleMessage)
}

const cafsCache = new Map<string, CafsFunctions>()
const cafsStoreCache = new Map<string, Cafs>()
const cafsLocker = new Map<string, number>()
const storeIndexCache = new Map<string, StoreIndex>()

function getStoreIndex (storeDir: string): StoreIndex {
  if (!storeIndexCache.has(storeDir)) {
    storeIndexCache.set(storeDir, new StoreIndex(storeDir))
  }
  return storeIndexCache.get(storeDir)!
}

async function handleMessage (
  message:
  | TarballExtractMessage
  | LinkPkgMessage
  | AddDirToStoreMessage
  | ReadPkgFromCafsMessage
  | SymlinkAllModulesMessage
  | HardLinkDirMessage
  | InitStoreMessage
  | WriteCafsFilesMessage
  | false
): Promise<void> {
  if (message === false) {
    parentPort!.off('message', handleMessage)
    // Explicitly close cached SQLite connections before exiting.
    // process.exit() in a worker thread may not run C++ destructors,
    // which would leave file descriptors and mmap regions open.
    for (const idx of storeIndexCache.values()) {
      idx.close()
    }
    storeIndexCache.clear()
    process.exit(0)
  }
  try {
    switch (message.type) {
      case 'extract': {
        parentPort!.postMessage(addTarballToStore(message))
        break
      }
      case 'link': {
        parentPort!.postMessage(importPackage(message))
        break
      }
      case 'add-dir': {
        parentPort!.postMessage(addFilesFromDir(message))
        break
      }
      case 'init-store': {
        parentPort!.postMessage(initStore(message))
        break
      }
      case 'readPkgFromCafs': {
        const { storeDir, filesIndexFile, verifyStoreIntegrity, expectedPkg, strictStorePkgContentCheck } = message
        const pkgFilesIndex = getStoreIndex(storeDir).get(filesIndexFile) as PackageFilesIndex | undefined
        if (!pkgFilesIndex) {
          parentPort!.postMessage({
            status: 'success',
            value: {
              verified: false,
              pkgFilesIndex: null,
            },
          })
          return
        }
        const warnings: string[] = []
        if (expectedPkg) {
          if (
            (
              pkgFilesIndex.manifest?.name != null &&
            expectedPkg.name != null &&
            pkgFilesIndex.manifest.name.toLowerCase() !== expectedPkg.name.toLowerCase()
            ) ||
          (
            pkgFilesIndex.manifest?.version != null &&
            expectedPkg.version != null &&
            !equalOrSemverEqual(pkgFilesIndex.manifest.version, expectedPkg.version)
          )
          ) {
            const msg = 'Package name or version mismatch found while reading from the store.'
            const hint = `This means that either the lockfile is broken or the package metadata (name and version) inside the package's package.json file doesn't match the metadata in the registry. Expected package: ${expectedPkg.name}@${expectedPkg.version}. Actual package in the store: ${pkgFilesIndex.manifest?.name}@${pkgFilesIndex.manifest?.version}.`
            if (strictStorePkgContentCheck ?? true) {
              throw new PnpmError('UNEXPECTED_PKG_CONTENT_IN_STORE', msg, {
                hint: `${hint}\n\nIf you want to ignore this issue, set strictStorePkgContentCheck to false in your configuration`,
              })
            } else {
              warnings.push(`${msg} ${hint}`)
            }
          }
        }
        let verifyResult: VerifyResult
        if (verifyStoreIntegrity) {
          verifyResult = checkPkgFilesIntegrity(storeDir, pkgFilesIndex)
        } else {
          verifyResult = buildFileMapsFromIndex(storeDir, pkgFilesIndex)
        }
        const bundledManifest = pkgFilesIndex.manifest
        const requiresBuild = pkgFilesIndex.requiresBuild ?? pkgRequiresBuild(bundledManifest, verifyResult.filesMap)

        parentPort!.postMessage({
          status: 'success',
          warnings,
          value: {
            verified: verifyResult.passed,
            bundledManifest,
            files: {
              filesMap: verifyResult.filesMap,
              sideEffectsMaps: verifyResult.sideEffectsMaps,
              resolvedFrom: 'store',
              requiresBuild,
            },
          },
        })
        break
      }
      case 'symlinkAllModules': {
        parentPort!.postMessage(symlinkAllModules(message))
        break
      }
      case 'hardLinkDir': {
        hardLinkDir(message.src, message.destDirs)
        parentPort!.postMessage({ status: 'success' })
        break
      }
      case 'write-cafs-files': {
        parentPort!.postMessage(await writeCafsFiles(message))
        break
      }
    }
  } catch (e: any) { // eslint-disable-line
    parentPort!.postMessage({
      status: 'error',
      error: {
        code: e.code,
        message: e.message ?? e.toString(),
        hint: e.hint,
      },
    })
  }
}

function addTarballToStore ({ buffer, storeDir, integrity, filesIndexFile, appendManifest, ignoreFilePattern }: TarballExtractMessage) {
  if (integrity) {
    const { algorithm, hexDigest } = parseIntegrity(integrity)
    const calculatedHash: string = crypto.hash(algorithm, buffer, 'hex')
    if (calculatedHash !== hexDigest) {
      return {
        status: 'error',
        error: {
          type: 'integrity_validation_failed',
          algorithm,
          expected: integrity,
          found: formatIntegrity(algorithm, calculatedHash),
        },
      }
    }
  }
  if (!cafsCache.has(storeDir)) {
    cafsCache.set(storeDir, createCafs(storeDir))
  }
  const cafs = cafsCache.get(storeDir)!
  const ignore = ignoreFilePattern ? makeIgnoreFromPattern(ignoreFilePattern) : undefined
  let { filesIndex, manifest } = cafs.addFilesFromTarball(buffer, true, ignore)
  if (appendManifest && manifest == null) {
    manifest = appendManifest
    addManifestToCafs(cafs, filesIndex, appendManifest)
  } else if (!filesIndex.has('package.json')) {
    addPlaceholderPackageJsonToCafs(cafs, filesIndex)
  }
  const { filesIntegrity, filesMap } = processFilesIndex(filesIndex)
  const bundledManifest = manifest != null ? normalizeBundledManifest(manifest) : undefined
  const requiresBuild = pkgRequiresBuild(bundledManifest, filesIntegrity)
  const pkgFilesIndex: PackageFilesIndex = {
    requiresBuild,
    manifest: bundledManifest,
    algo: HASH_ALGORITHM,
    files: filesIntegrity,
  }
  return {
    status: 'success',
    value: {
      filesMap,
      manifest: bundledManifest,
      requiresBuild,
      integrity: integrity ?? calcIntegrity(buffer),
    },
    indexWrites: [{ key: filesIndexFile, buffer: packToShared(pkgFilesIndex) }],
  }
}

function calcIntegrity (buffer: Buffer): string {
  const calculatedHash: string = crypto.hash('sha512', buffer, 'hex')
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
    const detail = util.types.isNativeError(err) ? `: ${err.message}` : ''
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
  }
  indexWrites?: IndexWrite[]
}

function initStore ({ storeDir }: InitStoreMessage): { status: string } {
  fs.mkdirSync(storeDir, { recursive: true })
  const hexChars = '0123456789abcdef'.split('')
  // Only create subdirectories for files/ — index/ is now managed by SQLite
  const filesDirPath = path.join(storeDir, 'files')
  try {
    fs.mkdirSync(filesDirPath)
  } catch {
    // If a parallel process has already started creating the directories in the store,
    // ignore if it already exists.
  }
  for (const hex1 of hexChars) {
    for (const hex2 of hexChars) {
      try {
        fs.mkdirSync(path.join(filesDirPath, `${hex1}${hex2}`))
      } catch {
        // If a parallel process has already started creating the directories in the store,
        // ignore if it already exists.
      }
    }
  }
  // The SQLite index database will be initialized lazily by getStoreIndex()
  // on the first operation that needs it (e.g., readPkgFromCafs, addFilesFromDir).
  // Eagerly opening it here races with the main thread's StoreIndex constructor,
  // which can cause SQLITE_CANTOPEN on Windows due to mandatory file locking.
  return { status: 'success' }
}

function addFilesFromDir (
  {
    appendManifest,
    dir,
    files,
    filesIndexFile,
    includeNodeModules,
    sideEffectsCacheKey,
    storeDir,
  }: AddDirToStoreMessage
): AddFilesFromDirResult {
  if (!cafsCache.has(storeDir)) {
    cafsCache.set(storeDir, createCafs(storeDir))
  }
  const cafs = cafsCache.get(storeDir)!
  let { filesIndex, manifest } = cafs.addFilesFromDir(dir, {
    files,
    includeNodeModules,
    readManifest: true,
  })
  if (appendManifest && manifest == null) {
    manifest = appendManifest
    addManifestToCafs(cafs, filesIndex, appendManifest)
  } else if (!filesIndex.has('package.json')) {
    addPlaceholderPackageJsonToCafs(cafs, filesIndex)
  }
  const { filesIntegrity, filesMap } = processFilesIndex(filesIndex)
  const bundledManifest = manifest != null ? normalizeBundledManifest(manifest) : undefined
  let requiresBuild: boolean
  let indexWrites: IndexWrite[] | undefined
  if (sideEffectsCacheKey) {
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
    if (!existingFilesIndex.sideEffects) {
      existingFilesIndex.sideEffects = new Map()
    }
    // Ensure side effects use the same algorithm as the original package
    if (existingFilesIndex.algo !== HASH_ALGORITHM) {
      throw new PnpmError(
        'ALGO_MISMATCH',
        `Algorithm mismatch: package index uses "${existingFilesIndex.algo}" but side effects were computed with "${HASH_ALGORITHM}"`
      )
    }
    existingFilesIndex.sideEffects.set(sideEffectsCacheKey, calculateDiff(existingFilesIndex.files, filesIntegrity))
    if (existingFilesIndex.requiresBuild == null) {
      requiresBuild = pkgRequiresBuild(manifest, filesMap)
    } else {
      requiresBuild = existingFilesIndex.requiresBuild
    }
    indexWrites = [{ key: filesIndexFile, buffer: packToShared(existingFilesIndex) }]
  } else {
    requiresBuild = pkgRequiresBuild(bundledManifest, filesIntegrity)
    const pkgFilesIndex: PackageFilesIndex = {
      requiresBuild,
      manifest: bundledManifest,
      algo: HASH_ALGORITHM,
      files: filesIntegrity,
    }
    indexWrites = [{ key: filesIndexFile, buffer: packToShared(pkgFilesIndex) }]
  }
  return { status: 'success', value: { filesMap, manifest: bundledManifest, requiresBuild }, indexWrites }
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
      baseFiles.get(file)!.mode !== sideEffectsFiles.get(file)!.mode
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
  for (const [k, { checkedAt, filePath, digest, mode, size }] of filesIndex) {
    filesIntegrity.set(k, {
      checkedAt,
      digest,
      mode,
      size,
    })
    filesMap.set(k, filePath)
  }
  return { filesIntegrity, filesMap }
}

interface ImportPackageResult {
  status: string
  value: {
    isBuilt: boolean
    importMethod?: string
  }
}

function importPackage ({
  storeDir,
  packageImportMethod,
  filesResponse,
  sideEffectsCacheKey,
  targetDir,
  requiresBuild,
  force,
  keepModulesDir,
  disableRelinkLocalDirDeps,
  safeToSkip,
}: LinkPkgMessage): ImportPackageResult {
  const cacheKey = JSON.stringify({ storeDir, packageImportMethod })
  if (!cafsStoreCache.has(cacheKey)) {
    cafsStoreCache.set(cacheKey, createCafsStore(storeDir, { packageImportMethod, cafsLocker }))
  }
  const cafsStore = cafsStoreCache.get(cacheKey)!
  const { importMethod, isBuilt } = cafsStore.importPackage(targetDir, {
    filesResponse,
    force,
    disableRelinkLocalDirDeps,
    requiresBuild,
    sideEffectsCacheKey,
    keepModulesDir,
    safeToSkip,
  })
  return { status: 'success', value: { isBuilt, importMethod } }
}

function symlinkAllModules (opts: SymlinkAllModulesMessage): { status: 'success' } {
  for (const dep of opts.deps) {
    for (const [alias, pkgDir] of Object.entries(dep.children)) {
      if (alias !== dep.name) {
        symlinkDependencySync(pkgDir, dep.modules, alias)
      }
    }
  }
  return { status: 'success' }
}

async function writeCafsFiles (message: WriteCafsFilesMessage): Promise<{ status: string, filesWritten: number }> {
  const { contentPathFromHex } = await import('@pnpm/store.cafs')

  // `message.payload` is the already-decompressed file portion of a
  // `/v1/install` response: a length-prefixed JSON header, then one
  // `[64-byte digest][u32 size][1-byte exec][content]` frame per file,
  // terminated by 64 zero bytes.
  const payload = Buffer.from(message.payload.buffer, message.payload.byteOffset, message.payload.byteLength)
  const END_MARKER = Buffer.alloc(64, 0)
  const createdDirs = new Set<string>()

  if (payload.length < 4) {
    throw new Error('pnpr server /v1/install file payload is truncated')
  }
  // Skip the length-prefixed JSON header that precedes the frames.
  const jsonLen = payload.readUInt32BE(0)
  let offset = 4 + jsonLen
  let filesWritten = 0
  let endMarkerSeen = false

  while (offset + 64 <= payload.length) {
    if (payload.subarray(offset, offset + 64).equals(END_MARKER)) {
      endMarkerSeen = true
      offset += 64
      break
    }
    if (offset + 69 > payload.length) break // 64 digest + 4 size + 1 mode
    const size = payload.readUInt32BE(offset + 64)
    const entryLen = 69 + size
    if (offset + entryLen > payload.length) break // incomplete entry

    const digest = payload.subarray(offset, offset + 64).toString('hex')
    const executable = (payload[offset + 68] & 0x01) !== 0
    const content = payload.subarray(offset + 69, offset + entryLen)

    const relPath = contentPathFromHex(executable ? 'exec' : 'nonexec', digest)
    const fullPath = path.join(message.storeDir, relPath)
    const dir = path.dirname(fullPath)
    if (!createdDirs.has(dir)) {
      fs.mkdirSync(dir, { recursive: true })
      createdDirs.add(dir)
    }
    try {
      fs.writeFileSync(fullPath, content, { flag: 'wx', mode: executable ? 0o755 : 0o644 })
    } catch (err: unknown) {
      if (!(err instanceof Error && 'code' in err && (err as NodeJS.ErrnoException).code === 'EEXIST')) {
        throw err
      }
      // EEXIST means the same digest is already at this CAFS path. CAFS is
      // content-addressed, so a complete file is by definition correct. But a
      // previous process could have crashed mid-write and left a truncated
      // file — the pnpr path skips integrity verification, so we'd silently
      // install garbage. Detect truncation by size and overwrite atomically.
      const onDiskSize = fs.statSync(fullPath).size
      if (onDiskSize !== content.length) {
        const tmpPath = `${fullPath}.tmp-${process.pid}-${Date.now()}`
        fs.writeFileSync(tmpPath, content, { mode: executable ? 0o755 : 0o644 })
        fs.renameSync(tmpPath, fullPath)
      }
    }
    filesWritten++
    offset += entryLen
  }

  if (!endMarkerSeen) {
    throw new Error('pnpr server /v1/install file payload ended without the end marker')
  }
  return { status: 'success', filesWritten }
}

