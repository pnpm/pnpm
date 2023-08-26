import path from 'path'
import fs from 'fs'
import gfs from '@pnpm/graceful-fs'
import * as crypto from 'crypto'
import { createCafsStore } from '@pnpm/create-cafs-store'
import {
  createCafs,
  type PackageFileInfo,
  type PackageFilesIndex,
  type FilesIndex,
  optimisticRenameOverwrite,
} from '@pnpm/store.cafs'
import { sync as loadJsonFile } from 'load-json-file'
import { parentPort } from 'worker_threads'
import { type TarballExtractMessage, type LinkPkgMessage, type AddDirToStoreMessage } from './types'

const INTEGRITY_REGEX: RegExp = /^([^-]+)-([A-Za-z0-9+/=]+)$/

parentPort!.on('message', handleMessage)

const cafsCache = new Map<string, ReturnType<typeof createCafs>>()
const cafsStoreCache = new Map<string, ReturnType<typeof createCafsStore>>()
const cafsLocker = new Map<string, number>()

async function handleMessage (message: TarballExtractMessage | LinkPkgMessage | AddDirToStoreMessage | false): Promise<void> {
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
    }
  } catch (e: any) { // eslint-disable-line
    parentPort!.postMessage({ status: 'error', error: e.toString() })
  }
}

function addTarballToStore ({ buffer, cafsDir, integrity, filesIndexFile }: TarballExtractMessage) {
  if (integrity) {
    const [, algo, integrityHash] = integrity.match(INTEGRITY_REGEX)!
    // Compensate for the possibility of non-uniform Base64 padding
    const normalizedRemoteHash: string = Buffer.from(integrityHash, 'base64').toString('hex')

    const calculatedHash: string = crypto.createHash(algo).update(buffer).digest('hex')
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
  if (!cafsCache.has(cafsDir)) {
    cafsCache.set(cafsDir, createCafs(cafsDir))
  }
  const cafs = cafsCache.get(cafsDir)!
  const { filesIndex, manifest } = cafs.addFilesFromTarball(buffer)
  const { filesIntegrity, filesMap } = processFilesIndex(filesIndex)
  writeFilesIndexFile(filesIndexFile, { pkg: manifest ?? {}, files: filesIntegrity })
  return { status: 'success', value: { filesIndex: filesMap, manifest } }
}

function addFilesFromDir ({ dir, cafsDir, filesIndexFile, sideEffectsCacheKey }: AddDirToStoreMessage) {
  if (!cafsCache.has(cafsDir)) {
    cafsCache.set(cafsDir, createCafs(cafsDir))
  }
  const cafs = cafsCache.get(cafsDir)!
  const { filesIndex, manifest } = cafs.addFilesFromDir(dir)
  const { filesIntegrity, filesMap } = processFilesIndex(filesIndex)
  if (sideEffectsCacheKey) {
    let filesIndex!: PackageFilesIndex
    try {
      filesIndex = loadJsonFile<PackageFilesIndex>(filesIndexFile)
    } catch {
      filesIndex = { files: filesIntegrity }
    }
    filesIndex.sideEffects = filesIndex.sideEffects ?? {}
    filesIndex.sideEffects[sideEffectsCacheKey] = filesIntegrity
    writeJsonFile(filesIndexFile, filesIndex)
  } else {
    writeFilesIndexFile(filesIndexFile, { pkg: manifest ?? {}, files: filesIntegrity })
  }
  return { status: 'success', value: { filesIndex: filesMap, manifest } }
}

function processFilesIndex (filesIndex: FilesIndex) {
  const filesIntegrity: Record<string, PackageFileInfo> = {}
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

function importPackage ({
  storeDir,
  packageImportMethod,
  filesResponse,
  sideEffectsCacheKey,
  targetDir,
  requiresBuild,
  force,
  keepModulesDir,
}: LinkPkgMessage) {
  const cacheKey = JSON.stringify({ storeDir, packageImportMethod })
  if (!cafsStoreCache.has(cacheKey)) {
    cafsStoreCache.set(cacheKey, createCafsStore(storeDir, { packageImportMethod, cafsLocker }))
  }
  const cafsStore = cafsStoreCache.get(cacheKey)!
  const { importMethod, isBuilt } = cafsStore.importPackage(targetDir, {
    filesResponse,
    force,
    requiresBuild,
    sideEffectsCacheKey,
    keepModulesDir,
  })
  return { status: 'success', value: { isBuilt, importMethod } }
}

function writeFilesIndexFile (
  filesIndexFile: string,
  { pkg, files }: {
    pkg: { name?: string, version?: string }
    files: Record<string, PackageFileInfo>
  }
) {
  writeJsonFile(filesIndexFile, {
    name: pkg.name,
    version: pkg.version,
    files,
  })
}

function writeJsonFile (filePath: string, data: unknown) {
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
