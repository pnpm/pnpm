import path from 'path'
import fs from 'fs'
import gfs from '@pnpm/graceful-fs'
import { type Cafs, type PackageFiles, type SideEffects, type SideEffectsDiff } from '@pnpm/cafs-types'
import { createCafsStore } from '@pnpm/create-cafs-store'
import * as crypto from '@pnpm/crypto.polyfill'
import { pkgRequiresBuild } from '@pnpm/exec.pkg-requires-build'
import { hardLinkDir } from '@pnpm/fs.hard-link-dir'
import {
  type CafsFunctions,
  checkPkgFilesIntegrity,
  createCafs,
  type PackageFilesIndex,
  type FilesIndex,
  optimisticRenameOverwrite,
  readManifestFromStore,
  type VerifyResult,
} from '@pnpm/store.cafs'
import { symlinkDependencySync } from '@pnpm/symlink-dependency'
import { type DependencyManifest } from '@pnpm/types'
import { sync as loadJsonFile } from 'load-json-file'
import { parentPort } from 'worker_threads'
import {
  type AddDirToStoreMessage,
  type ReadPkgFromCafsMessage,
  type LinkPkgMessage,
  type SymlinkAllModulesMessage,
  type TarballExtractMessage,
  type HardLinkDirMessage,
  type InitStoreMessage,
} from './types'

const INTEGRITY_REGEX: RegExp = /^([^-]+)-([A-Za-z0-9+/=]+)$/

parentPort!.on('message', handleMessage)

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
      let { storeDir, filesIndexFile, readManifest, verifyStoreIntegrity } = message
      let pkgFilesIndex: PackageFilesIndex | undefined
      try {
        pkgFilesIndex = loadJsonFile<PackageFilesIndex>(filesIndexFile)
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
      let verifyResult: VerifyResult | undefined
      if (pkgFilesIndex.requiresBuild == null) {
        readManifest = true
      }
      if (verifyStoreIntegrity) {
        verifyResult = checkPkgFilesIntegrity(storeDir, pkgFilesIndex, readManifest)
      } else {
        verifyResult = {
          passed: true,
          manifest: readManifest ? readManifestFromStore(storeDir, pkgFilesIndex) : undefined,
        }
      }
      const requiresBuild = pkgFilesIndex.requiresBuild ?? pkgRequiresBuild(verifyResult.manifest, pkgFilesIndex.files)
      parentPort!.postMessage({
        status: 'success',
        value: {
          verified: verifyResult.passed,
          manifest: verifyResult.manifest,
          pkgFilesIndex,
          requiresBuild,
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
      },
    })
  }
}

function addTarballToStore ({ buffer, storeDir, integrity, filesIndexFile }: TarballExtractMessage) {
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
  const { filesIndex, manifest } = cafs.addFilesFromTarball(buffer, true)
  const { filesIntegrity, filesMap } = processFilesIndex(filesIndex)
  const requiresBuild = writeFilesIndexFile(filesIndexFile, { manifest: manifest ?? {}, files: filesIntegrity })
  return { status: 'success', value: { filesIndex: filesMap, manifest, requiresBuild } }
}

interface AddFilesFromDirResult {
  status: string
  value: {
    filesIndex: Record<string, string>
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

function addFilesFromDir ({ dir, storeDir, filesIndexFile, sideEffectsCacheKey, files }: AddDirToStoreMessage): AddFilesFromDirResult {
  if (!cafsCache.has(storeDir)) {
    cafsCache.set(storeDir, createCafs(storeDir))
  }
  const cafs = cafsCache.get(storeDir)!
  const { filesIndex, manifest } = cafs.addFilesFromDir(dir, {
    files,
    readManifest: true,
  })
  const { filesIntegrity, filesMap } = processFilesIndex(filesIndex)
  let requiresBuild: boolean
  if (sideEffectsCacheKey) {
    let filesIndex!: PackageFilesIndex
    try {
      filesIndex = loadJsonFile<PackageFilesIndex>(filesIndexFile)
    } catch {
      // If there is no existing index file, then we cannot store the side effects.
      return {
        status: 'success',
        value: {
          filesIndex: filesMap,
          manifest,
          requiresBuild: pkgRequiresBuild(manifest, filesIntegrity),
        },
      }
    }
    filesIndex.sideEffects = filesIndex.sideEffects ?? {}
    filesIndex.sideEffects[sideEffectsCacheKey] = calculateDiff(filesIndex.files, filesIntegrity)
    if (filesIndex.requiresBuild == null) {
      requiresBuild = pkgRequiresBuild(manifest, filesIntegrity)
    } else {
      requiresBuild = filesIndex.requiresBuild
    }
    writeJsonFile(filesIndexFile, filesIndex)
  } else {
    requiresBuild = writeFilesIndexFile(filesIndexFile, { manifest: manifest ?? {}, files: filesIntegrity })
  }
  return { status: 'success', value: { filesIndex: filesMap, manifest, requiresBuild } }
}

function calculateDiff (baseFiles: PackageFiles, sideEffectsFiles: PackageFiles): SideEffectsDiff {
  const deleted: string[] = []
  const added: PackageFiles = {}
  for (const file of new Set([...Object.keys(baseFiles), ...Object.keys(sideEffectsFiles)])) {
    if (!sideEffectsFiles[file]) {
      deleted.push(file)
    } else if (
      !baseFiles[file] ||
      baseFiles[file].integrity !== sideEffectsFiles[file].integrity ||
      baseFiles[file].mode !== sideEffectsFiles[file].mode
    ) {
      added[file] = sideEffectsFiles[file]
    }
  }
  const diff: SideEffectsDiff = {}
  if (deleted.length > 0) {
    diff.deleted = deleted
  }
  if (Object.keys(added).length > 0) {
    diff.added = added
  }
  return diff
}

interface ProcessFilesIndexResult {
  filesIntegrity: PackageFiles
  filesMap: Record<string, string>
}

function processFilesIndex (filesIndex: FilesIndex): ProcessFilesIndexResult {
  const filesIntegrity: PackageFiles = {}
  const filesMap: Record<string, string> = {}
  for (const [k, { checkedAt, filePath, integrity, mode, size }] of Object.entries(filesIndex)) {
    filesIntegrity[k] = {
      checkedAt,
      integrity: integrity.toString(), // TODO: use the raw Integrity object
      mode,
      size,
    }
    filesMap[k] = filePath
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
  writeJsonFile(filesIndexFile, filesIndex)
  return requiresBuild
}

function writeJsonFile (filePath: string, data: unknown): void {
  const targetDir = path.dirname(filePath)
  // TODO: use the API of @pnpm/cafs to write this file
  // There is actually no need to create the directory in 99% of cases.
  // So by using cafs API, we'll improve performance.
  fs.mkdirSync(targetDir, { recursive: true })
  // We remove the "-index.json" from the end of the temp file name
  // in order to avoid ENAMETOOLONG errors
  const temp = `${filePath.slice(0, -11)}${process.pid}`
  gfs.writeFileSync(temp, JSON.stringify(data))
  optimisticRenameOverwrite(temp, filePath)
}

process.on('uncaughtException', (err) => {
  console.error(err)
})
