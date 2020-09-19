import { DeferredManifestPromise } from '@pnpm/fetcher-base'
import { PackageFileInfo } from '@pnpm/store-controller-types'
import { parseJsonBuffer } from './parseJson'
import { getFilePathByModeInCafs } from './getFilePathInCafs'
import rimraf = require('@zkochan/rimraf')
import fs = require('mz/fs')
import pLimit = require('p-limit')
import ssri = require('ssri')

const limit = pLimit(20)
const MAX_BULK_SIZE = 1 * 1024 * 1024 // 1MB

export interface PackageFilesIndex {
  files: Record<string, PackageFileInfo>
  sideEffects?: Record<string, Record<string, PackageFileInfo>>
}

export default async function (
  cafsDir: string,
  pkgIndex: Record<string, PackageFileInfo>,
  manifest?: DeferredManifestPromise
) {
  let verified = true
  await Promise.all(
    Object.keys(pkgIndex)
      .map((f) =>
        limit(async () => {
          const fstat = pkgIndex[f]
          if (!fstat.integrity) {
            throw new Error(`Integrity checksum is missing for ${f}`)
          }
          if (
            !await verifyFile(
              getFilePathByModeInCafs(cafsDir, fstat.integrity, fstat.mode),
              fstat,
              f === 'package.json' ? manifest : undefined
            )
          ) {
            verified = false
          }
        })
      )
  )
  return verified
}

type FileInfo = Pick<PackageFileInfo, 'size' | 'checkedAt'> & {
  integrity: string | ssri.IntegrityLike
}

async function verifyFile (
  filename: string,
  fstat: FileInfo,
  deferredManifest?: DeferredManifestPromise
) {
  const currentFile = await checkFile(filename, fstat.checkedAt)
  if (!currentFile) return false
  if (currentFile.isModified) {
    if (currentFile.size !== fstat.size) {
      await rimraf(filename)
      return false
    }
    return verifyFileIntegrity(filename, fstat, deferredManifest)
  }
  if (deferredManifest) {
    parseJsonBuffer(await fs.readFile(filename), deferredManifest)
  }
  // If a file was not edited, we are skipping integrity check.
  // We assume that nobody will manually remove a file in the store and create a new one.
  return true
}

export async function verifyFileIntegrity (
  filename: string,
  expectedFile: FileInfo,
  deferredManifest?: DeferredManifestPromise
) {
  try {
    if (expectedFile.size > MAX_BULK_SIZE && !deferredManifest) {
      const ok = Boolean(await ssri.checkStream(fs.createReadStream(filename), expectedFile.integrity))
      if (!ok) {
        await rimraf(filename)
      }
      return ok
    }
    const data = await fs.readFile(filename)
    const ok = Boolean(ssri.checkData(data, expectedFile.integrity))
    if (!ok) {
      await rimraf(filename)
    } else if (deferredManifest) {
      parseJsonBuffer(data, deferredManifest)
    }
    return ok
  } catch (err) {
    switch (err.code) {
    case 'ENOENT': return false
    case 'EINTEGRITY': {
      // Broken files are removed from the store
      await rimraf(filename)
      return false
    }
    }
    throw err
  }
}

async function checkFile (filename: string, checkedAt?: number) {
  try {
    const { mtimeMs, size } = await fs.stat(filename)
    return {
      isModified: (mtimeMs - (checkedAt ?? 0)) > 100,
      size,
    }
  } catch (err) {
    if (err.code === 'ENOENT') return null
    throw err
  }
}
