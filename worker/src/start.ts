import crypto from 'crypto'
import path from 'path'
import fs from 'fs'
import { PnpmError } from '@pnpm/error'
import { type Cafs, type PackageFiles, type SideEffects, type SideEffectsDiff, type FilesMap } from '@pnpm/cafs-types'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { pkgRequiresBuild } from '@pnpm/exec.pkg-requires-build'
import { hardLinkDir } from '@pnpm/fs.hard-link-dir'
import { readMsgpackFileSync, writeMsgpackFileSync } from '@pnpm/fs.msgpack-file'
import { formatIntegrity, parseIntegrity } from '@pnpm/crypto.integrity'
import {
  type CafsFunctions,
  checkPkgFilesIntegrity,
  buildFileMapsFromIndex,
  createCafs,
  HASH_ALGORITHM,
  normalizeBundledManifest,
  type PackageFilesIndex,
  type FilesIndex,
  optimisticRenameOverwrite,
  type VerifyResult,
} from '@pnpm/store.cafs'
import { symlinkDependencySync } from '@pnpm/symlink-dependency'
import { type BundledManifest, type DependencyManifest } from '@pnpm/types'
import { parentPort } from 'worker_threads'
import { equalOrSemverEqual } from './equalOrSemverEqual.js'
import {
  type AddDirToStoreMessage,
  type ReadPkgFromCafsMessage,
  type LinkPkgMessage,
  type SymlinkAllModulesMessage,
  type TarballExtractMessage,
  type HardLinkDirMessage,
  type InitStoreMessage,
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

async function handleMessage (
  message:
  | TarballExtractMessage
  | LinkPkgMessage
  | AddDirToStoreMessage
  | ReadPkgFromCafsMessage
  | SymlinkAllModulesMessage
  | HardLinkDirMessage
  | InitStoreMessage
  | false
): Promise<void> {
  if (message === false) {
    parentPort!.off('message', handleMessage)
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
      let pkgFilesIndex: PackageFilesIndex | undefined
      try {
        pkgFilesIndex = readMsgpackFileSync<PackageFilesIndex>(filesIndexFile)
      } catch {
        // ignoring. It is fine if the integrity file is not present. Just refetch the package
      }
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

function addTarballToStore ({ buffer, storeDir, integrity, filesIndexFile, appendManifest }: TarballExtractMessage) {
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
  let { filesIndex, manifest } = cafs.addFilesFromTarball(buffer, true)
  if (appendManifest && manifest == null) {
    manifest = appendManifest
    addManifestToCafs(cafs, filesIndex, appendManifest)
  }
  const { filesIntegrity, filesMap } = processFilesIndex(filesIndex)
  const bundledManifest = manifest != null ? normalizeBundledManifest(manifest) : undefined
  const requiresBuild = writeFilesIndexFile(filesIndexFile, { algo: HASH_ALGORITHM, manifest: bundledManifest, files: filesIntegrity })
  return {
    status: 'success',
    value: {
      filesMap,
      manifest: bundledManifest,
      requiresBuild,
      integrity: integrity ?? calcIntegrity(buffer),
    },
  }
}

function calcIntegrity (buffer: Buffer): string {
  const calculatedHash: string = crypto.hash('sha512', buffer, 'hex')
  return formatIntegrity('sha512', calculatedHash)
}

interface AddFilesFromDirResult {
  status: string
  value: {
    filesMap: FilesMap
    manifest?: BundledManifest
    requiresBuild: boolean
  }
}

function initStore ({ storeDir }: InitStoreMessage): { status: string } {
  fs.mkdirSync(storeDir, { recursive: true })
  const hexChars = '0123456789abcdef'.split('')
  for (const subDir of ['files', 'index']) {
    const subDirPath = path.join(storeDir, subDir)
    try {
      fs.mkdirSync(subDirPath)
    } catch {
      // If a parallel process has already started creating the directories in the store,
      // ignore if it already exists.
    }
    for (const hex1 of hexChars) {
      for (const hex2 of hexChars) {
        try {
          fs.mkdirSync(path.join(subDirPath, `${hex1}${hex2}`))
        } catch {
          // If a parallel process has already started creating the directories in the store,
          // ignore if it already exists.
        }
      }
    }
  }
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
  }
  const { filesIntegrity, filesMap } = processFilesIndex(filesIndex)
  const bundledManifest = manifest != null ? normalizeBundledManifest(manifest) : undefined
  let requiresBuild: boolean
  if (sideEffectsCacheKey) {
    let existingFilesIndex!: PackageFilesIndex
    try {
      existingFilesIndex = readMsgpackFileSync<PackageFilesIndex>(filesIndexFile)
    } catch {
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
    writeIndexFile(filesIndexFile, existingFilesIndex)
  } else {
    requiresBuild = writeFilesIndexFile(filesIndexFile, { algo: HASH_ALGORITHM, manifest: bundledManifest, files: filesIntegrity })
  }
  return { status: 'success', value: { filesMap, manifest: bundledManifest, requiresBuild } }
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

function writeFilesIndexFile (
  filesIndexFile: string,
  { algo, manifest, files, sideEffects }: {
    algo: string
    manifest?: BundledManifest
    files: PackageFiles
    sideEffects?: SideEffects
  }
): boolean {
  const requiresBuild = pkgRequiresBuild(manifest, files)
  const filesIndex: PackageFilesIndex = {
    requiresBuild,
    manifest,
    algo,
    files,
    sideEffects,
  }
  writeIndexFile(filesIndexFile, filesIndex)
  return requiresBuild
}

function writeIndexFile (filePath: string, data: PackageFilesIndex): void {
  const targetDir = path.dirname(filePath)
  // TODO: use the API of @pnpm/cafs to write this file
  // There is actually no need to create the directory in 99% of cases.
  // So by using cafs API, we'll improve performance.
  fs.mkdirSync(targetDir, { recursive: true })
  // Drop the last 10 characters and append the PID to create a shorter unique temp filename.
  // This avoids ENAMETOOLONG errors on systems with path length limits.
  const temp = `${filePath.slice(0, -10)}${process.pid}`
  writeMsgpackFileSync(temp, data)
  optimisticRenameOverwrite(temp, filePath)
}