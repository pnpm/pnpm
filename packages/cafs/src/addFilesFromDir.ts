import { promises as fs } from 'fs'
import path from 'path'
import {
  DeferredManifestPromise,
  FilesIndex,
  FileWriteResult,
} from '@pnpm/fetcher-base'
import gfs from '@pnpm/graceful-fs'
import pLimit from 'p-limit'
import { parseJsonBuffer } from './parseJson'

const limit = pLimit(20)

const MAX_BULK_SIZE = 1 * 1024 * 1024 // 1MB

export default async function (
  cafs: {
    addStream: (stream: NodeJS.ReadableStream, mode: number) => Promise<FileWriteResult>
    addBuffer: (buffer: Buffer, mode: number) => Promise<FileWriteResult>
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
    addBuffer: (buffer: Buffer, mode: number) => Promise<FileWriteResult>
  },
  rootDir: string,
  currDir: string,
  index: FilesIndex,
  deferredManifest?: DeferredManifestPromise
) {
  try {
    const files = await fs.readdir(currDir)
    await Promise.all(files.map(async (file) => {
      const fullPath = path.join(currDir, file)
      const stat = await fs.stat(fullPath)
      if (stat.isDirectory()) {
        await _retrieveFileIntegrities(cafs, rootDir, fullPath, index)
        return
      }
      if (stat.isFile()) {
        const relativePath = path.relative(rootDir, fullPath)
        const writeResult = limit(async () => {
          if ((deferredManifest != null) && rootDir === currDir && file === 'package.json') {
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
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') {
      throw err
    }
  }
}
