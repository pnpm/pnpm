import getStream = require('get-stream')
import path = require('path')
import exists = require('path-exists')
import pathTemp = require('path-temp')
import renameOverwrite = require('rename-overwrite')
import ssri = require('ssri')
import { Hash } from 'ssri'
import addFilesFromDir from './addFilesFromDir'
import addFilesFromTarball from './addFilesFromTarball'
import checkFilesIntegrity from './checkFilesIntegrity'
import writeFile from './writeFile'

export { checkFilesIntegrity }

export default function createCafs (cafsDir: string, ignore?: ((filename: string) => Boolean)) {
  const locker = new Map()
  const addStream = addStreamToCafs.bind(null, locker, cafsDir)
  const addBuffer = addBufferToCafs.bind(null, locker, cafsDir)
  return {
    addFilesFromDir: addFilesFromDir.bind(null, { addBuffer, addStream }),
    addFilesFromTarball: addFilesFromTarball.bind(null, addStream, ignore ?? null),
  }
}

async function addStreamToCafs (
  locker: Map<string, Promise<void>>,
  cafsDir: string,
  fileStream: NodeJS.ReadableStream,
  mode: number,
): Promise<ssri.Integrity> {
  const buffer = await getStream.buffer(fileStream)
  return addBufferToCafs(locker, cafsDir, buffer, mode)
}

const modeIsExecutable = (mode: number) => (mode & 0o111) === 0o111

async function addBufferToCafs (
  locker: Map<string, Promise<void>>,
  cafsDir: string,
  buffer: Buffer,
  mode: number,
): Promise<ssri.Integrity> {
  const integrity = ssri.fromData(buffer)
  const isExecutable = modeIsExecutable(mode)
  const fileDest = contentPathFromHex(cafsDir, isExecutable, integrity.hexDigest())
  if (locker.has(fileDest)) {
    await locker.get(fileDest)
    return integrity
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
    await writeFile(temp, buffer, isExecutable ? 0o755 : undefined)
    await renameOverwrite(temp, fileDest)
  })()
  locker.set(fileDest, p)
  await p
  return integrity
}

export function getFilePathInCafs (
  cafsDir: string,
  file: {
    integrity: string | Hash,
    mode: number,
  },
) {
  return contentPathFromIntegrity(cafsDir, file.integrity, file.mode)
}

function contentPathFromIntegrity (
  cafsDir: string,
  integrity: string | Hash,
  mode: number,
) {
  const sri = ssri.parse(integrity, { single: true })
  const isExecutable = modeIsExecutable(mode)
  return contentPathFromHex(cafsDir, isExecutable, sri.hexDigest())
}

function contentPathFromHex (cafsDir: string, isExecutable: boolean, hex: string) {
  return path.join(
    isExecutable ? path.join(cafsDir, 'x') : cafsDir,
    hex.slice(0, 2),
    hex.slice(2),
  )
}
