import fs, { type Stats } from 'fs'
import path from 'path'
import type {
  DeferredManifestPromise,
  FilesIndex,
  FileWriteResult,
} from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { parseJsonBuffer } from './parseJson'

export function addFilesFromDir (
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  dirname: string,
  manifest?: DeferredManifestPromise
): FilesIndex {
  const index: FilesIndex = {}
  _retrieveFileIntegrities(addBuffer, dirname, dirname, index, manifest)
  if (manifest && !index['package.json']) {
    manifest.resolve(undefined)
  }
  return index
}

function _retrieveFileIntegrities (
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  rootDir: string,
  currDir: string,
  index: FilesIndex,
  deferredManifest?: DeferredManifestPromise
) {
  const files = fs.readdirSync(currDir, { withFileTypes: true })
  for (const file of files) {
    const fullPath = path.join(currDir, file.name)
    if (file.isDirectory()) {
      _retrieveFileIntegrities(addBuffer, rootDir, fullPath, index)
      continue
    }
    if (file.isFile()) {
      const relativePath = path.relative(rootDir, fullPath)
      let stat: Stats
      try {
        stat = fs.statSync(fullPath)
      } catch (err: any) { // eslint-disable-line
        if (err.code !== 'ENOENT') {
          throw err
        }
        continue
      }
      const writeResult = (() => {
        if ((deferredManifest != null) && rootDir === currDir && file.name === 'package.json') {
          const buffer = gfs.readFileSync(fullPath)
          parseJsonBuffer(buffer, deferredManifest)
          return addBuffer(buffer, stat.mode)
        }
        const buffer = gfs.readFileSync(fullPath)
        return addBuffer(buffer, stat.mode)
      })()
      index[relativePath] = {
        mode: stat.mode,
        size: stat.size,
        ...writeResult,
      }
    }
  }
}
