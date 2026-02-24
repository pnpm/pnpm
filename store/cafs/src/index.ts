import crypto from 'crypto'
import {
  type AddToStoreResult,
  type FileWriteResult,
  type PackageFiles,
  type PackageFileInfo,
  type FilesIndex,
  type SideEffects,
  type SideEffectsDiff,
} from '@pnpm/cafs-types'
import { addFilesFromDir } from './addFilesFromDir.js'
import { addFilesFromTarball } from './addFilesFromTarball.js'
import {
  checkPkgFilesIntegrity,
  buildFileMapsFromIndex,
  type Integrity,
  type PackageFilesIndex,
  type VerifyResult,
} from './checkPkgFilesIntegrity.js'
import {
  getIndexFilePathInCafs,
  contentPathFromHex,
  type FileType,
  getFilePathByModeInCafs,
  modeIsExecutable,
} from './getFilePathInCafs.js'
import { normalizeBundledManifest } from './normalizeBundledManifest.js'
import { optimisticRenameOverwrite, writeBufferToCafs } from './writeBufferToCafs.js'

export const HASH_ALGORITHM = 'sha512'

export { type BundledManifest } from '@pnpm/types'
export { normalizeBundledManifest }

export {
  checkPkgFilesIntegrity,
  buildFileMapsFromIndex,
  type FileType,
  getFilePathByModeInCafs,
  getIndexFilePathInCafs,
  type Integrity,
  type PackageFileInfo,
  type PackageFiles,
  type PackageFilesIndex,
  optimisticRenameOverwrite,
  type FilesIndex,
  type VerifyResult,
  type SideEffects,
  type SideEffectsDiff,
}

export type CafsLocker = Map<string, number>

export interface CreateCafsOpts {
  ignoreFile?: (filename: string) => boolean
  cafsLocker?: CafsLocker
}

export interface CafsFunctions {
  addFilesFromDir: (dirname: string, opts?: { files?: string[], readManifest?: boolean, includeNodeModules?: boolean }) => AddToStoreResult
  addFilesFromTarball: (tarballBuffer: Buffer, readManifest?: boolean) => AddToStoreResult
  addFile: (buffer: Buffer, mode: number) => FileWriteResult
  getIndexFilePathInCafs: (integrity: string, pkgId: string) => string
  getFilePathByModeInCafs: (digest: string, mode: number) => string
}

export function createCafs (storeDir: string, { ignoreFile, cafsLocker }: CreateCafsOpts = {}): CafsFunctions {
  const _writeBufferToCafs = writeBufferToCafs.bind(null, cafsLocker ?? new Map(), storeDir)
  const addBuffer = addBufferToCafs.bind(null, _writeBufferToCafs)
  return {
    addFilesFromDir: addFilesFromDir.bind(null, addBuffer),
    addFilesFromTarball: addFilesFromTarball.bind(null, addBuffer, ignoreFile ?? null),
    addFile: addBuffer,
    getIndexFilePathInCafs: getIndexFilePathInCafs.bind(null, storeDir),
    getFilePathByModeInCafs: getFilePathByModeInCafs.bind(null, storeDir),
  }
}

type WriteBufferToCafs = (buffer: Buffer, fileDest: string, mode: number | undefined, integrity: Integrity) => { checkedAt: number, filePath: string }

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
    isExecutable ? 0o755 : undefined,
    { digest, algorithm: HASH_ALGORITHM }
  )
  return { checkedAt, filePath, digest }
}
