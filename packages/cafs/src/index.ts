import { PackageFileInfo } from '@pnpm/store-controller-types'
import addFilesFromDir from './addFilesFromDir'
import addFilesFromTarball from './addFilesFromTarball'
import checkFilesIntegrity, {
  PackageFilesIndex,
} from './checkFilesIntegrity'
import getFilePathInCafs, {
  contentPathFromHex,
  FileType,
  getFilePathByModeInCafs,
  modeIsExecutable,
} from './getFilePathInCafs'
import writeFile from './writeFile'
import path = require('path')
import getStream = require('get-stream')
import exists = require('path-exists')
import pathTemp = require('path-temp')
import renameOverwrite = require('rename-overwrite')
import ssri = require('ssri')

export {
  checkFilesIntegrity,
  FileType,
  getFilePathByModeInCafs,
  getFilePathInCafs,
  PackageFileInfo,
  PackageFilesIndex,
}

export default function createCafs (cafsDir: string, ignore?: ((filename: string) => Boolean)) {
  const locker = new Map()
  const _writeBufferToCafs = writeBufferToCafs.bind(null, locker, cafsDir)
  const addStream = addStreamToCafs.bind(null, _writeBufferToCafs)
  const addBuffer = addBufferToCafs.bind(null, _writeBufferToCafs)
  return {
    addFilesFromDir: addFilesFromDir.bind(null, { addBuffer, addStream }),
    addFilesFromTarball: addFilesFromTarball.bind(null, addStream, ignore ?? null),
  }
}

async function addStreamToCafs (
  writeBufferToCafs: WriteBufferToCafs,
  fileStream: NodeJS.ReadableStream,
  mode: number
): Promise<ssri.Integrity> {
  const buffer = await getStream.buffer(fileStream)
  return addBufferToCafs(writeBufferToCafs, buffer, mode)
}

type WriteBufferToCafs = (buffer: Buffer, fileDest: string, mode: number | undefined) => Promise<void>

async function addBufferToCafs (
  writeBufferToCafs: WriteBufferToCafs,
  buffer: Buffer,
  mode: number
): Promise<ssri.Integrity> {
  const integrity = ssri.fromData(buffer)
  const isExecutable = modeIsExecutable(mode)
  const fileDest = contentPathFromHex(isExecutable ? 'exec' : 'nonexec', integrity.hexDigest())
  await writeBufferToCafs(buffer, fileDest, isExecutable ? 0o755 : undefined)
  return integrity
}

async function writeBufferToCafs (
  locker: Map<string, Promise<void>>,
  cafsDir: string,
  buffer: Buffer,
  fileDest: string,
  mode: number | undefined
) {
  fileDest = path.join(cafsDir, fileDest)
  if (locker.has(fileDest)) {
    await locker.get(fileDest)
    return
  }
  const p = (async () => {
    // This is a slow operation. Should be rewritten
    if (await exists(fileDest)) return

    // This might be too cautious.
    // The write is atomic, so in case pnpm crashes, no broken file
    // will be added to the store.
    // It might be a redundant step though, as we verify the contents of the
    // files before linking
    //
    // If we don't allow --no-verify-store-integrity then we probably can write
    // to the final file directly.
    const temp = pathTemp(path.dirname(fileDest))
    await writeFile(temp, buffer, mode)
    await renameOverwrite(temp, fileDest)
  })()
  locker.set(fileDest, p)
  await p
}
