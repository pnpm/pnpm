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
  opts: {
    files?: string[]
    readManifest?: boolean
  } = {}
): AddToStoreResult {
  const filesIndex: FilesIndex = {}
  let manifest: DependencyManifest | undefined
  const files = opts.files ?? findFilesInDir(dirname)
  for (const file of files) {
    const fullPath = path.join(dirname, file)
    const relativePath = path.relative(dirname, fullPath)
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
    if (opts.readManifest && file === 'package.json') {
      manifest = parseJsonBufferSync(buffer)
    }
    filesIndex[relativePath] = {
      mode: stat.mode,
      size: stat.size,
      ...addBuffer(buffer, stat.mode),
    }
  }
  return { manifest, filesIndex }
}

function findFilesInDir (dir: string): string[] {
  const files: string[] = []
  findFiles(files, dir)
  return files
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
