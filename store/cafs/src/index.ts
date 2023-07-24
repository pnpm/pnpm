import type { FileWriteResult, PackageFileInfo } from '@pnpm/cafs-types'
import getStream from 'get-stream'
import ssri from 'ssri'
import { addFilesFromDir } from './addFilesFromDir'
import { addFilesFromTarball } from './addFilesFromTarball'
import {
  checkPkgFilesIntegrity,
  type PackageFilesIndex,
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
}

export type CafsLocker = Map<string, number>

export interface CreateCafsOpts {
  ignoreFile?: (filename: string) => boolean
  cafsLocker?: CafsLocker
}

export function createCafs (cafsDir: string, { ignoreFile, cafsLocker }: CreateCafsOpts = {}) {
  const _writeBufferToCafs = writeBufferToCafs.bind(null, cafsLocker ?? new Map(), cafsDir)
  const addStream = addStreamToCafs.bind(null, _writeBufferToCafs)
  const addBuffer = addBufferToCafs.bind(null, _writeBufferToCafs)
  return {
    addFilesFromDir: addFilesFromDir.bind(null, { addBuffer, addStream }),
    addFilesFromTarball: addFilesFromTarball.bind(null, addStream, ignoreFile ?? null),
    getFilePathInCafs: getFilePathInCafs.bind(null, cafsDir),
    getFilePathByModeInCafs: getFilePathByModeInCafs.bind(null, cafsDir),
  }
}

async function addStreamToCafs (
  writeBufferToCafs: WriteBufferToCafs,
  fileStream: NodeJS.ReadableStream,
  mode: number
): Promise<FileWriteResult> {
  const buffer = await getStream.buffer(fileStream)
  return addBufferToCafs(writeBufferToCafs, buffer, mode)
}

type WriteBufferToCafs = (buffer: Buffer, fileDest: string, mode: number | undefined, integrity: ssri.IntegrityLike) => Promise<number>

async function addBufferToCafs (
  writeBufferToCafs: WriteBufferToCafs,
  buffer: Buffer,
  mode: number
): Promise<FileWriteResult> {
  // Calculating the integrity of the file is surprisingly fast.
  // 30K files are calculated in 1 second.
  // Hence, from a performance perspective, there is no win in fetching the package index file from the registry.
  const integrity = ssri.fromData(buffer)
  const isExecutable = modeIsExecutable(mode)
  const fileDest = contentPathFromHex(isExecutable ? 'exec' : 'nonexec', integrity.hexDigest())
  const checkedAt = await writeBufferToCafs(
    buffer,
    fileDest,
    isExecutable ? 0o755 : undefined,
    integrity
  )
  return { checkedAt, integrity }
}
