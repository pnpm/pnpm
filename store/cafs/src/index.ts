import '@total-typescript/ts-reset'

import ssri from 'ssri'

import type { AddToStoreResult, CreateCafsOpts, FileType, FileWriteResult, WriteBufferToCafs } from '@pnpm/types'

import {
  modeIsExecutable,
  getFilePathInCafs,
  contentPathFromHex,
  getFilePathByModeInCafs,
} from './getFilePathInCafs.js'
import { addFilesFromDir } from './addFilesFromDir.js'
import { writeBufferToCafs } from './writeBufferToCafs.js'
import { addFilesFromTarball } from './addFilesFromTarball.js'

export type { IntegrityLike } from 'ssri'

export { getFilePathInCafs, getFilePathByModeInCafs }
export { readManifestFromStore } from './readManifestFromStore.js'
export { optimisticRenameOverwrite } from './writeBufferToCafs.js'
export { checkPkgFilesIntegrity } from './checkPkgFilesIntegrity.js'

export function createCafs(
  cafsDir: string,
  { ignoreFile, cafsLocker }: CreateCafsOpts | undefined = {}
): {
    addFilesFromDir: (dirname: string, readManifest?: boolean | undefined) => AddToStoreResult;
    addFilesFromTarball: (tarballBuffer: Buffer, readManifest?: boolean | undefined) => AddToStoreResult;
    getFilePathInCafs: (integrity: string | ssri.IntegrityLike, fileType: FileType) => string;
    getFilePathByModeInCafs: (integrity: string | ssri.IntegrityLike, mode: number) => string;
  } {
  const _writeBufferToCafs = writeBufferToCafs.bind(
    null,
    cafsLocker ?? new Map(),
    cafsDir
  )

  const addBuffer = addBufferToCafs.bind(null, _writeBufferToCafs)

  return {
    addFilesFromDir: addFilesFromDir.bind(null, addBuffer),
    addFilesFromTarball: addFilesFromTarball.bind(
      null,
      addBuffer,
      ignoreFile ?? null
    ),
    getFilePathInCafs: getFilePathInCafs.bind(null, cafsDir),
    getFilePathByModeInCafs: getFilePathByModeInCafs.bind(null, cafsDir),
  }
}

function addBufferToCafs(
  writeBufferToCafs: WriteBufferToCafs,
  buffer: Buffer,
  mode: number
): FileWriteResult {
  // Calculating the integrity of the file is surprisingly fast.
  // 30K files are calculated in 1 second.
  // Hence, from a performance perspective, there is no win in fetching the package index file from the registry.
  const integrity = ssri.fromData(buffer)

  const isExecutable = modeIsExecutable(mode)

  const fileDest = contentPathFromHex(
    isExecutable ? 'exec' : 'nonexec',
    integrity.hexDigest()
  )

  const { checkedAt, filePath } = writeBufferToCafs(
    buffer,
    fileDest,
    isExecutable ? 0o7_5_5 : undefined,
    integrity
  )

  return { checkedAt, integrity, filePath }
}
