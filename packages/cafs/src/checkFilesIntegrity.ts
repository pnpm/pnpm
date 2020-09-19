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

async function verifyFile (
  filename: string,
  fstat: PackageFileInfo,
  deferredManifest?: DeferredManifestPromise
) {
  const modified = await isModified(filename, fstat.birthtimeMs)
  if (!deferredManifest && !modified) {
    // If a file was not edited, we are skipping integrity check.
    // We assume that nobody will manually remove a file in the store and create a new one.
    return true
  }
  if (fstat.size > MAX_BULK_SIZE && !deferredManifest) {
    try {
      const ok = Boolean(await ssri.checkStream(fs.createReadStream(filename), fstat.integrity))
      if (!ok) {
        await rimraf(filename)
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

  try {
    const data = await fs.readFile(filename)
    const ok = !modified || Boolean(ssri.checkData(data, fstat.integrity))
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

async function isModified (filename: string, birthtimeMs?: number) {
  const { mtimeMs } = await fs.stat(filename)
  return (mtimeMs - (birthtimeMs ?? 0)) > 100
}
