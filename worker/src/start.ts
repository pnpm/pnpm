import crypto from 'crypto'
import path from 'path'
import fs from 'fs'
import { PnpmError } from '@pnpm/error'
import { type Cafs, type PackageFiles, type SideEffects, type SideEffectsDiff, type FilesMap } from '@pnpm/cafs-types'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { pkgRequiresBuild } from '@pnpm/exec.pkg-requires-build'
import { hardLinkDir } from '@pnpm/fs.hard-link-dir'
import { readMsgpackFileSync, writeMsgpackFileSync } from '@pnpm/fs.msgpack-file'
import {
  type CafsFunctions,
  checkPkgFilesIntegrity,
  buildFileMapsFromIndex,
  createCafs,
  type PackageFilesIndex,
  type FilesIndex,
  optimisticRenameOverwrite,
  type VerifyResult,
} from '@pnpm/store.cafs'
import { symlinkDependencySync } from '@pnpm/symlink-dependency'
import { type DependencyManifest } from '@pnpm/types'
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

const INTEGRITY_REGEX: RegExp = /^([^-]+)-([a-z0-9+/=]+)$/i

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
      let { storeDir, filesIndexFile, readManifest, verifyStoreIntegrity, expectedPkg, strictStorePkgContentCheck } = message
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
            pkgFilesIndex.name != null &&
            expectedPkg.name != null &&
            pkgFilesIndex.name.toLowerCase() !== expectedPkg.name.toLowerCase()
          ) ||
          (
            pkgFilesIndex.version != null &&
            expectedPkg.version != null &&
            !equalOrSemverEqual(pkgFilesIndex.version, expectedPkg.version)
          )
        ) {
          const msg = 'Package name or version mismatch found while reading from the store.'
          const hint = `This means that either the lockfile is broken or the package metadata (name and version) inside the package's package.json file doesn't match the metadata in the registry. Expected package: ${expectedPkg.name}@${expectedPkg.version}. Actual package in the store: ${pkgFilesIndex.name}@${pkgFilesIndex.version}.`
          if (strictStorePkgContentCheck ?? true) {
            throw new PnpmError('UNEXPECTED_PKG_CONTENT_IN_STORE', msg, {
              hint: `${hint}\n\nIf you want to ignore this issue, set strictStorePkgContentCheck to false in your configuration`,
            })
          } else {
            warnings.push(`${msg} ${hint}`)
          }
        }
      }
      let verifyResult: VerifyResult | undefined
      if (pkgFilesIndex.requiresBuild == null) {
        readManifest = true
      }
      // Get file maps and optionally verify
      if (verifyStoreIntegrity) {
        verifyResult = checkPkgFilesIntegrity(storeDir, pkgFilesIndex, readManifest)
      } else {
        verifyResult = buildFileMapsFromIndex(storeDir, pkgFilesIndex, readManifest)
      }
      const requiresBuild = pkgFilesIndex.requiresBuild ?? pkgRequiresBuild(verifyResult.manifest, verifyResult.filesMap)

      parentPort!.postMessage({
        status: 'success',
        warnings,
        value: {
          verified: verifyResult.passed,
          manifest: verifyResult.manifest,
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
    const [, algo, integrityHash] = integrity.match(INTEGRITY_REGEX)!
    // Compensate for the possibility of non-uniform Base64 padding
    const normalizedRemoteHash: string = Buffer.from(integrityHash, 'base64').toString('hex')

    const calculatedHash: string = crypto.hash(algo, buffer, 'hex')
    if (calculatedHash !== normalizedRemoteHash) {
      return {
        status: 'error',
        error: {
          type: 'integrity_validation_failed',
          algorithm: algo,
          expected: integrity,
          found: `${algo}-${Buffer.from(calculatedHash, 'hex').toString('base64')}`,
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
  const requiresBuild = writeFilesIndexFile(filesIndexFile, { manifest: manifest ?? {}, files: filesIntegrity })
  return {
    status: 'success',
    value: {
      filesMap,
      manifest,
      requiresBuild,
      integrity: integrity ?? calcIntegrity(buffer),
    },
  }
}

function calcIntegrity (buffer: Buffer): string {
  const calculatedHash: string = crypto.hash('sha512', buffer, 'hex')
  return `sha512-${Buffer.from(calculatedHash, 'hex').toString('base64')}`
}

interface AddFilesFromDirResult {
  status: string
  value: {
    filesMap: FilesMap
    manifest?: DependencyManifest
    requiresBuild: boolean
  }
}

function initStore ({ storeDir }: InitStoreMessage): { status: string } {
  fs.mkdirSync(storeDir, { recursive: true })
  try {
    const hexChars = '0123456789abcdef'.split('')
    for (const subDir of ['files', 'index']) {
      const subDirPath = path.join(storeDir, subDir)
      fs.mkdirSync(subDirPath)
      for (const hex1 of hexChars) {
        for (const hex2 of hexChars) {
          fs.mkdirSync(path.join(subDirPath, `${hex1}${hex2}`))
        }
      }
    }
  } catch {
    // If a parallel process has already started creating the directories in the store,
    // then we just stop.
  }
  return { status: 'success' }
}

function addFilesFromDir (
  {
    appendManifest,
    dir,
    files,
    filesIndexFile,
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
    readManifest: true,
  })
  if (appendManifest && manifest == null) {
    manifest = appendManifest
    addManifestToCafs(cafs, filesIndex, appendManifest)
  }
  const { filesIntegrity, filesMap } = processFilesIndex(filesIndex)
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
          manifest,
          requiresBuild: pkgRequiresBuild(manifest, filesMap),
        },
      }
    }
    if (!existingFilesIndex.sideEffects) {
      existingFilesIndex.sideEffects = new Map()
    }
    existingFilesIndex.sideEffects.set(sideEffectsCacheKey, calculateDiff(existingFilesIndex.files, filesIntegrity))
    if (existingFilesIndex.requiresBuild == null) {
      requiresBuild = pkgRequiresBuild(manifest, filesMap)
    } else {
      requiresBuild = existingFilesIndex.requiresBuild
    }
    writeIndexFile(filesIndexFile, existingFilesIndex)
  } else {
    requiresBuild = writeFilesIndexFile(filesIndexFile, { manifest: manifest ?? {}, files: filesIntegrity })
  }
  return { status: 'success', value: { filesMap, manifest, requiresBuild } }
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
      baseFiles.get(file)!.integrity !== sideEffectsFiles.get(file)!.integrity ||
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
  for (const [k, { checkedAt, filePath, integrity, mode, size }] of filesIndex) {
    filesIntegrity.set(k, {
      checkedAt,
      integrity: integrity.toString(), // TODO: use the raw Integrity object
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
  { manifest, files, sideEffects }: {
    manifest: Partial<DependencyManifest>
    files: PackageFiles
    sideEffects?: SideEffects
  }
): boolean {
  const requiresBuild = pkgRequiresBuild(manifest, files)
  const filesIndex: PackageFilesIndex = {
    name: manifest.name,
    version: manifest.version,
    requiresBuild,
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
