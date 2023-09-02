import fs, { type Stats } from 'fs'
import path from 'path'
import {
  type AddToStoreResult,
  type FilesIndex,
  type FileWriteResult,
} from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { type DependencyManifest } from '@pnpm/types'
import { parseJsonBufferSync } from './parseJson'

export function addFilesFromDir (
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  dirname: string,
  readManifest?: boolean
): AddToStoreResult {
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
      const buffer = gfs.readFileSync(fullPath)
      if (rootDir === currDir && readManifest && file.name === 'package.json') {
        manifest = parseJsonBufferSync(buffer)
      }
      index[relativePath] = {
        mode: stat.mode,
        size: stat.size,
        ...addBuffer(buffer, stat.mode),
      }
    }
  }
  return manifest
}
