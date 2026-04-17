import crypto from 'node:crypto'

import type {
  AddToStoreResult,
  FilesIndex,
  FileWriteResult,
  PackageFileInfo,
  PackageFiles,
  SideEffects,
  SideEffectsDiff,
} from '@pnpm/store.cafs-types'

import { addFilesFromDir } from './addFilesFromDir.js'
import { addFilesFromTarball } from './addFilesFromTarball.js'
import {
  buildFileMapsFromIndex,
  checkPkgFilesIntegrity,
  type Integrity,
  type PackageFilesIndex,
  type VerifyResult,
} from './checkPkgFilesIntegrity.js'
import {
  contentPathFromHex,
  type FileType,
  getFilePathByModeInCafs,
  modeIsExecutable,
} from './getFilePathInCafs.js'
import { createJsonParseCache, type JsonParseCache } from './jsonCache.js'
import { normalizeBundledManifest } from './normalizeBundledManifest.js'
import { writeBufferToCafs } from './writeBufferToCafs.js'

export const HASH_ALGORITHM = 'sha512'

export { type BundledManifest } from '@pnpm/types'
export { createJsonParseCache, type JsonParseCache }
export { normalizeBundledManifest }

export {
  buildFileMapsFromIndex,
  checkPkgFilesIntegrity,
  type FilesIndex,
  type FileType,
  getFilePathByModeInCafs,
  type Integrity,
  type PackageFileInfo,
  type PackageFiles,
  type PackageFilesIndex,
  type SideEffects,
  type SideEffectsDiff,
  type VerifyResult,
}

export type CafsLocker = Map<string, number>

export interface CreateCafsOpts {
  ignoreFile?: (filename: string) => boolean
  cafsLocker?: CafsLocker
  jsonCache?: JsonParseCache
}

export interface CafsFunctions {
  addFilesFromDir: (dirname: string, opts?: { files?: string[], readManifest?: boolean, includeNodeModules?: boolean }) => AddToStoreResult
  addFilesFromTarball: (tarballBuffer: Buffer, readManifest?: boolean) => AddToStoreResult
  addFile: (buffer: Buffer, mode: number) => FileWriteResult
  getFilePathByModeInCafs: (digest: string, mode: number) => string
}

export function createCafs (storeDir: string, { ignoreFile, cafsLocker, jsonCache }: CreateCafsOpts = {}): CafsFunctions {
  const _writeBufferToCafs = writeBufferToCafs.bind(null, cafsLocker ?? new Map(), storeDir)
  const addBuffer = addBufferToCafs.bind(null, _writeBufferToCafs)
  return {
    addFilesFromDir: addFilesFromDir.bind(null, addBuffer, jsonCache),
    addFilesFromTarball: addFilesFromTarball.bind(null, addBuffer, ignoreFile ?? null, jsonCache),
    addFile: addBuffer,
    getFilePathByModeInCafs: getFilePathByModeInCafs.bind(null, storeDir),
  }
}

type WriteBufferToCafs = (buffer: Buffer, fileDest: string, mode: number | undefined) => { checkedAt: number, filePath: string }

function addBufferToCafs (
  writeBufferToCafs: WriteBufferToCafs,
  buffer: Buffer,
  mode: number
): FileWriteResult {
  // Calculating the integrity of the file is surprisingly fast.
  // 30K files are calculated in 1 second.
  // Hence, from a performance perspective, there is no win in fetching the package index file from the registry.
  const digest = crypto.hash(HASH_ALGORITHM, buffer, 'hex')
  const isExecutable = modeIsExecutable(mode)
  const fileDest = contentPathFromHex(isExecutable ? 'exec' : 'nonexec', digest)
  const { checkedAt, filePath } = writeBufferToCafs(
    buffer,
    fileDest,
    isExecutable ? 0o755 : undefined
  )
  return { checkedAt, filePath, digest }
}
