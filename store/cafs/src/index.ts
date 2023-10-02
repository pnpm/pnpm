import { type FileWriteResult, type PackageFileInfo, type FilesIndex } from '@pnpm/cafs-types'
import ssri from 'ssri'
import { addFilesFromDir } from './addFilesFromDir'
import { addFilesFromTarball } from './addFilesFromTarball'
import {
  checkPkgFilesIntegrity,
  type PackageFilesIndex,
  type VerifyResult,
} from './checkPkgFilesIntegrity'
import { readManifestFromStore } from './readManifestFromStore'
import {
  getFilePathInCafs,
  contentPathFromHex,
  type FileType,
  getFilePathByModeInCafs,
  modeIsExecutable,
} from './getFilePathInCafs'
import { optimisticRenameOverwrite, writeBufferToCafs } from './writeBufferToCafs'

export type { IntegrityLike } from 'ssri'

export {
  checkPkgFilesIntegrity,
  readManifestFromStore,
  type FileType,
  getFilePathByModeInCafs,
  getFilePathInCafs,
  type PackageFileInfo,
  type PackageFilesIndex,
  optimisticRenameOverwrite,
  type FilesIndex,
  type VerifyResult,
}

export type CafsLocker = Map<string, number>

export interface CreateCafsOpts {
  ignoreFile?: (filename: string) => boolean
  cafsLocker?: CafsLocker
}

export function createCafs (cafsDir: string, { ignoreFile, cafsLocker }: CreateCafsOpts = {}) {
  const _writeBufferToCafs = writeBufferToCafs.bind(null, cafsLocker ?? new Map(), cafsDir)
  const addBuffer = addBufferToCafs.bind(null, _writeBufferToCafs)
  return {
    addFilesFromDir: addFilesFromDir.bind(null, addBuffer),
    addFilesFromTarball: addFilesFromTarball.bind(null, addBuffer, ignoreFile ?? null),
    getFilePathInCafs: getFilePathInCafs.bind(null, cafsDir),
    getFilePathByModeInCafs: getFilePathByModeInCafs.bind(null, cafsDir),
  }
}

type WriteBufferToCafs = (buffer: Buffer, fileDest: string, mode: number | undefined, integrity: ssri.IntegrityLike) => { checkedAt: number, filePath: string }

function addBufferToCafs (
  writeBufferToCafs: WriteBufferToCafs,
  buffer: Buffer,
  mode: number
): FileWriteResult {
  // Calculating the integrity of the file is surprisingly fast.
  // 30K files are calculated in 1 second.
  // Hence, from a performance perspective, there is no win in fetching the package index file from the registry.
  const integrity = ssri.fromData(buffer)
  const isExecutable = modeIsExecutable(mode)
  const fileDest = contentPathFromHex(isExecutable ? 'exec' : 'nonexec', integrity.hexDigest())
  const { checkedAt, filePath } = writeBufferToCafs(
    buffer,
    fileDest,
    isExecutable ? 0o755 : undefined,
    integrity
  )
  return { checkedAt, integrity, filePath }
}
