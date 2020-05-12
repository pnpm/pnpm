import { DeferredManifestPromise, FilesIndex } from '@pnpm/fetcher-base'
import fs = require('mz/fs')
import pLimit from 'p-limit'
import path = require('path')
import ssri = require('ssri')
import { parseJsonBuffer } from './parseJson'

const limit = pLimit(20)

const MAX_BULK_SIZE = 1 * 1024 * 1024 // 1MB

export default async function (
  cafs: {
    addStream: (stream: NodeJS.ReadableStream, mode: number) => Promise<ssri.Integrity>,
    addBuffer: (buffer: Buffer, mode: number) => Promise<ssri.Integrity>,
  },
  dirname: string,
  manifest?: DeferredManifestPromise
) {
  const index = {}
  await _retrieveFileIntegrities(cafs, dirname, dirname, index, manifest)
  return index
}

async function _retrieveFileIntegrities (
  cafs: {
    addStream: (stream: NodeJS.ReadableStream, mode: number) => Promise<ssri.Integrity>,
    addBuffer: (buffer: Buffer, mode: number) => Promise<ssri.Integrity>,
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
        const generatingIntegrity = limit(async () => {
          if (deferredManifest && rootDir === currDir && file === 'package.json') {
            const buffer = await fs.readFile(fullPath)
            parseJsonBuffer(buffer, deferredManifest)
            return cafs.addBuffer(buffer, stat.mode)
          }
          if (stat.size < MAX_BULK_SIZE) {
            const buffer = await fs.readFile(fullPath)
            return cafs.addBuffer(buffer, stat.mode)
          }
          return cafs.addStream(fs.createReadStream(fullPath), stat.mode)
        })
        index[relativePath] = {
          generatingIntegrity,
          mode: stat.mode,
          size: stat.size,
        }
      }
    }))
  } catch (err) {
    if (err.code !== 'ENOENT') {
      throw err
    }
  }
}
