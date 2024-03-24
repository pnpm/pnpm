import path from 'node:path'
import fs, { type Stats } from 'node:fs'

import type {
  FilesIndex,
  FileWriteResult,
  AddToStoreResult,
  DependencyManifest,
} from '@pnpm/types'
import gfs from '@pnpm/graceful-fs'

import { parseJsonBufferSync } from './parseJson.js'

export function addFilesFromDir(
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  dirname: string,
  readManifest?: boolean | undefined
): AddToStoreResult {
  const filesIndex: FilesIndex = {}

  const manifest = _retrieveFileIntegrities(
    addBuffer,
    dirname,
    dirname,
    filesIndex,
    readManifest
  )

  return { filesIndex, manifest }
}

function _retrieveFileIntegrities(
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  rootDir: string,
  currDir: string,
  index: FilesIndex,
  readManifest?: boolean | undefined
): DependencyManifest | undefined {
  const files = fs.readdirSync(currDir, { withFileTypes: true })

  let manifest: DependencyManifest | undefined

  for (const file of files) {
    const fullPath = path.join(currDir, file.name)

    if (file.isDirectory()) {
      _retrieveFileIntegrities(addBuffer, rootDir, fullPath, index)
      continue
    }

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

    if (stat.isDirectory()) {
      _retrieveFileIntegrities(addBuffer, rootDir, fullPath, index)
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

  return manifest
}
