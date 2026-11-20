import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import type { FileWriteResult } from '@pnpm/store.cafs-types'
import { grantModeBits, mkdirInheritingMode, readDirMode, unixCreationMode } from '@pnpm/store.file-mode'

import { verifyFileIntegrityAsync } from './checkPkgFilesIntegrity.js'
import { getFilePathByModeInCafs, modeIsExecutable } from './getFilePathInCafs.js'
import type { TarballFileWriter } from './parseTarball.js'
import { closeQuietly, openStoreFileForRepair, optimisticRenameOverwrite } from './writeBufferToCafs.js'

export interface TarballFileWriterFactory {
  create: (mode: number, onFile: (file: FileWriteResult) => void) => TarballFileWriter
  publish: () => Promise<void>
  cleanup: () => void
}

export function createTarballFileWriterFactory (storeDir: string): TarballFileWriterFactory {
  const state: { temporaryDirectory?: string, sequence: number } = { sequence: 0 }
  const descriptors = new Set<number>()
  const pending: Array<{ filename: string, filePath: string, mode: number, digest: string }> = []
  return {
    create: (mode, onFile) => {
      if (state.temporaryDirectory == null) {
        mkdirInheritingMode(storeDir)
        state.temporaryDirectory = fs.mkdtempSync(path.join(storeDir, 'tarball-'))
      }
      const filename = path.join(state.temporaryDirectory, String(state.sequence++))
      const descriptor = fs.openSync(filename, 'wx', 0o600)
      descriptors.add(descriptor)
      const hash = crypto.createHash('sha512')
      return {
        write: (chunk) => {
          hash.update(chunk)
          writeChunk(descriptor, chunk)
        },
        end: () => {
          fs.closeSync(descriptor)
          descriptors.delete(descriptor)
          const digest = hash.digest('hex')
          const filePath = getFilePathByModeInCafs(storeDir, digest, mode)
          pending.push({ filename, filePath, mode, digest })
          onFile({ digest, filePath, checkedAt: Date.now() })
        },
      }
    },
    publish: async () => {
      for (const file of pending) {
        // eslint-disable-next-line no-await-in-loop -- a digest is reused after its first publication
        await reuseOrPublishFile(file)
      }
    },
    cleanup: () => {
      for (const descriptor of descriptors) fs.closeSync(descriptor)
      descriptors.clear()
      if (state.temporaryDirectory) fs.rmSync(state.temporaryDirectory, { recursive: true, force: true })
    },
  }
}

function writeChunk (descriptor: number, chunk: Buffer): void {
  let written = 0
  while (written < chunk.length) written += fs.writeSync(descriptor, chunk, written, chunk.length - written)
}

async function reuseOrPublishFile (file: { filename: string, filePath: string, mode: number, digest: string }): Promise<void> {
  const integrity = { algorithm: 'sha512', digest: file.digest }
  if (await verifyFileIntegrityAsync(file.filePath, integrity)) return
  if (await repairFileInPlace(file.filename, file.filePath, integrity)) return
  publishFile(file.filename, file.filePath, file.mode)
}

async function repairFileInPlace (filename: string, filePath: string, integrity: { algorithm: string, digest: string }): Promise<boolean> {
  const descriptor = openStoreFileForRepair(filePath)
  if (descriptor == null) return false
  try {
    fs.ftruncateSync(descriptor, 0)
    for await (const chunk of fs.createReadStream(filename)) writeChunk(descriptor, chunk as Buffer)
  } catch {
    return false
  } finally {
    closeQuietly(descriptor)
  }
  return verifyFileIntegrityAsync(filePath, integrity)
}

function publishFile (filename: string, filePath: string, mode: number): void {
  const directory = path.dirname(filePath)
  mkdirInheritingMode(directory)
  if (process.platform !== 'win32') {
    const creation = unixCreationMode(readDirMode(directory), modeIsExecutable(mode) ? 0o755 : undefined)
    fs.chmodSync(filename, creation.openMode ?? 0o666 & ~process.umask())
    if (creation.grantMode != null) {
      const descriptor = fs.openSync(filename, 'r+')
      try {
        grantModeBits(descriptor, creation.grantMode)
      } finally {
        fs.closeSync(descriptor)
      }
    }
  }
  optimisticRenameOverwrite(filename, filePath)
}
