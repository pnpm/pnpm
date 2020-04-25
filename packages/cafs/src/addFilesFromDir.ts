import { FilesIndex } from '@pnpm/fetcher-base'
import fs = require('mz/fs')
import pLimit from 'p-limit'
import path = require('path')
import ssri = require('ssri')

const limit = pLimit(20)

const MAX_BULK_SIZE = 1 * 1024 * 1024 // 1MB

export default async function (
  cafs: {
    addStream: (stream: NodeJS.ReadableStream) => Promise<ssri.Integrity>,
    addBuffer: (buffer: Buffer) => Promise<ssri.Integrity>,
  },
  dirname: string,
) {
  const index = {}
  await _retrieveFileIntegrities(cafs, dirname, dirname, index)
  return index
}

async function _retrieveFileIntegrities (
  cafs: {
    addStream: (stream: NodeJS.ReadableStream) => Promise<ssri.Integrity>,
    addBuffer: (buffer: Buffer) => Promise<ssri.Integrity>,
  },
  rootDir: string,
  currDir: string,
  index: FilesIndex,
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
        index[relativePath] = {
          generatingIntegrity: limit(() => {
            return stat.size < MAX_BULK_SIZE
              ? fs.readFile(fullPath).then(cafs.addBuffer)
              : cafs.addStream(fs.createReadStream(fullPath))
          }),
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
