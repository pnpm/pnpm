import fs, { type Stats } from 'fs'
import path from 'path'
import type {
  FilesIndex,
  FileWriteResult,
} from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { type DependencyManifest } from '@pnpm/types'
import { parseJsonBufferSync } from './parseJson'

export function addFilesFromDir (
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  dirname: string,
  readManifest?: boolean
): { filesIndex: FilesIndex, manifest?: DependencyManifest } {
  const filesIndex: FilesIndex = {}
  const manifest = _retrieveFileIntegrities(addBuffer, dirname, dirname, filesIndex, readManifest)
  return { filesIndex, manifest }
}

function _retrieveFileIntegrities (
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  rootDir: string,
  currDir: string,
  index: FilesIndex,
  readManifest?: boolean
) {
  const files = fs.readdirSync(currDir, { withFileTypes: true })
  let manifest: DependencyManifest | undefined
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
        if (readManifest && rootDir === currDir && file.name === 'package.json') {
          const buffer = gfs.readFileSync(fullPath)
          manifest = parseJsonBufferSync(buffer)
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
  return manifest
}
