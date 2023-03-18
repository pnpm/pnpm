import { promises as fs, type Stats } from 'fs'
import path from 'path'
import type { FileWriteResult, PackageFileInfo } from '@pnpm/cafs-types'
import getStream from 'get-stream'
import pathTemp from 'path-temp'
import renameOverwrite from 'rename-overwrite'
import ssri from 'ssri'
import { addFilesFromDir } from './addFilesFromDir'
import { addFilesFromTarball } from './addFilesFromTarball'
import {
  checkPkgFilesIntegrity,
  type PackageFilesIndex,
  verifyFileIntegrity,
} from './checkPkgFilesIntegrity'
import { readManifestFromStore } from './readManifestFromStore'
import {
  getFilePathInCafs,
  contentPathFromHex,
  type FileType,
  getFilePathByModeInCafs,
  modeIsExecutable,
} from './getFilePathInCafs'
import { writeFile } from './writeFile'

export type { IntegrityLike } from 'ssri'

export {
  checkPkgFilesIntegrity,
  readManifestFromStore,
  type FileType,
  getFilePathByModeInCafs,
  getFilePathInCafs,
  type PackageFileInfo,
  type PackageFilesIndex,
}

export function createCafs (cafsDir: string, ignore?: ((filename: string) => boolean)) {
  const locker = new Map()
  const _writeBufferToCafs = writeBufferToCafs.bind(null, locker, cafsDir)
  const addStream = addStreamToCafs.bind(null, _writeBufferToCafs)
  const addBuffer = addBufferToCafs.bind(null, _writeBufferToCafs)
  return {
    addFilesFromDir: addFilesFromDir.bind(null, { addBuffer, addStream }),
    addFilesFromTarball: addFilesFromTarball.bind(null, addStream, ignore ?? null),
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

async function writeBufferToCafs (
  locker: Map<string, Promise<number>>,
  cafsDir: string,
  buffer: Buffer,
  fileDest: string,
  mode: number | undefined,
  integrity: ssri.IntegrityLike
): Promise<number> {
  fileDest = path.join(cafsDir, fileDest)
  if (locker.has(fileDest)) {
    return locker.get(fileDest)!
  }
  const p = (async () => {
    // This part is a bit redundant.
    // When a file is already used by another package,
    // we probably have validated its content already.
    // However, there is no way to find which package index file references
    // the given file. So we should revalidate the content of the file again.
    if (await existsSame(fileDest, integrity)) {
      return Date.now()
    }

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
    // Unfortunately, "birth time" (time of file creation) is available not on all filesystems.
    // We log the creation time ourselves and save it in the package index file.
    // Having this information allows us to skip content checks for files that were not modified since "birth time".
    const birthtimeMs = Date.now()
    await renameOverwrite(temp, fileDest)
    return birthtimeMs
  })()
  locker.set(fileDest, p)
  return p
}

async function existsSame (filename: string, integrity: ssri.IntegrityLike) {
  let existingFile: Stats | undefined
  try {
    existingFile = await fs.stat(filename)
  } catch (err) {
    return false
  }
  return verifyFileIntegrity(filename, {
    size: existingFile.size,
    integrity,
  })
}
