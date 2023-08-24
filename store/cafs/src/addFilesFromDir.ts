import { promises as fs, type Stats } from 'fs'
import path from 'path'
import type {
  DeferredManifestPromise,
  FilesIndex,
  FileWriteResult,
} from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import pLimit from 'p-limit'
import { parseJsonBuffer } from './parseJson'

const limit = pLimit(20)

const MAX_BULK_SIZE = 1 * 1024 * 1024 // 1MB

export async function addFilesFromDir (
  cafs: {
    addStream: (stream: NodeJS.ReadableStream, mode: number) => Promise<FileWriteResult>
    addBuffer: (buffer: Buffer, mode: number) => FileWriteResult
  },
  dirname: string,
  manifest?: DeferredManifestPromise
): Promise<FilesIndex> {
  const index: FilesIndex = {}
  await _retrieveFileIntegrities(cafs, dirname, dirname, index, manifest)
  if (manifest && !index['package.json']) {
    manifest.resolve(undefined)
  }
  return index
}

async function _retrieveFileIntegrities (
  cafs: {
    addStream: (stream: NodeJS.ReadableStream, mode: number) => Promise<FileWriteResult>
    addBuffer: (buffer: Buffer, mode: number) => FileWriteResult
  },
  rootDir: string,
  currDir: string,
  index: FilesIndex,
  deferredManifest?: DeferredManifestPromise
) {
  const files = await fs.readdir(currDir, { withFileTypes: true })
  await Promise.all(files.map(async (file) => {
    const fullPath = path.join(currDir, file.name)
    if (file.isDirectory()) {
      await _retrieveFileIntegrities(cafs, rootDir, fullPath, index)
      return
    }
    if (file.isFile()) {
      const relativePath = path.relative(rootDir, fullPath)
      let stat: Stats
      try {
        stat = await fs.stat(fullPath)
      } catch (err: any) { // eslint-disable-line
        if (err.code !== 'ENOENT') {
          throw err
        }
        return
      }
      const writeResult = limit(async () => {
        if ((deferredManifest != null) && rootDir === currDir && file.name === 'package.json') {
          const buffer = await gfs.readFile(fullPath)
          parseJsonBuffer(buffer, deferredManifest)
          return cafs.addBuffer(buffer, stat.mode)
        }
        if (stat.size < MAX_BULK_SIZE) {
          const buffer = await gfs.readFile(fullPath)
          return cafs.addBuffer(buffer, stat.mode)
        }
        return cafs.addStream(gfs.createReadStream(fullPath), stat.mode)
      })
      index[relativePath] = {
        mode: stat.mode,
        size: stat.size,
        writeResult,
      }
    }
  }))
}
