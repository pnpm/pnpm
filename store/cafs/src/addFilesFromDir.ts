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
  files?: string[],
  readManifest?: boolean
): AddToStoreResult {
  const filesIndex: FilesIndex = {}
  const manifest = _retrieveFileIntegrities(addBuffer, dirname, filesIndex, files, readManifest)
  return { filesIndex, manifest }
}

function findFiles (
  filesList: string[],
  dir: string,
  relativeDir = ''
) {
  const files = fs.readdirSync(dir, { withFileTypes: true })
  for (const file of files) {
    const relativeSubdir = `${relativeDir}${relativeDir ? '/' : ''}${file.name}`
    if (file.isDirectory()) {
      findFiles(filesList, path.join(dir, file.name), relativeSubdir)
    } else {
      filesList.push(relativeSubdir)
    }
  }
}

function _retrieveFileIntegrities (
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  rootDir: string,
  index: FilesIndex,
  files?: string[],
  readManifest?: boolean
) {
  let manifest: DependencyManifest | undefined
  let filesList = files
  if (!filesList) {
    filesList = []
    findFiles(filesList, rootDir)
  }
  for (const file of filesList) {
    const fullPath = path.join(rootDir, file)
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
    if (readManifest && file === 'package.json') {
      manifest = parseJsonBufferSync(buffer)
    }
    index[relativePath] = {
      mode: stat.mode,
      size: stat.size,
      ...addBuffer(buffer, stat.mode),
    }
  }
  return manifest
}
