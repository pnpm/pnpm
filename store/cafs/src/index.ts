import '@total-typescript/ts-reset'

import ssri from 'ssri'

import { CreateCafsOpts, FileWriteResult, WriteBufferToCafs } from '@pnpm/types'

import {
  modeIsExecutable,
  getFilePathInCafs,
  contentPathFromHex,
  getFilePathByModeInCafs,
} from './getFilePathInCafs'
import {
  writeBufferToCafs,
} from './writeBufferToCafs'
import { addFilesFromDir } from './addFilesFromDir'
import { addFilesFromTarball } from './addFilesFromTarball'

export type { IntegrityLike } from 'ssri'

export { getFilePathInCafs, getFilePathByModeInCafs }

export function createCafs(
  cafsDir: string,
  { ignoreFile, cafsLocker }: CreateCafsOpts = {}
) {
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
