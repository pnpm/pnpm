import path from 'node:path'

import { formatIntegrity } from '@pnpm/crypto.integrity'
import * as dp from '@pnpm/dependency-path'
import { getContextForSingleImporter } from '@pnpm/get-context'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  type PackageSnapshot,
} from '@pnpm/lockfile.utils'
import { streamParser } from '@pnpm/logger'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { gitHostedStoreIndexKey, storeIndexKey } from '@pnpm/store.index'
import { StoreIndex } from '@pnpm/store.index'
import type { TarballResolution } from '@pnpm/store-controller-types'
import type { DepPath } from '@pnpm/types'
import dint from 'dint'
import pFilter from 'p-filter'

import {
  extendStoreStatusOptions,
  type StoreStatusOptions,
} from './extendStoreStatusOptions.js'

export async function storeStatus (maybeOpts: StoreStatusOptions): Promise<string[]> {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendStoreStatusOptions(maybeOpts)
  const {
    storeDir,
    skipped,
    virtualStoreDir,
    wantedLockfile,
  } = await getContextForSingleImporter({}, {
    ...opts,
    extraBinPaths: [], // ctx.extraBinPaths is not needed, so this is fine
  })
  if (!wantedLockfile) return []

  const pkgs = (Object.entries(wantedLockfile.packages ?? {}) as Array<[DepPath, PackageSnapshot]>)
    .filter(([depPath]) => !skipped.has(depPath))
    .map(([depPath, pkgSnapshot]) => {
      const id = packageIdFromSnapshot(depPath, pkgSnapshot)
      return {
        depPath,
        id,
        integrity: (pkgSnapshot.resolution as TarballResolution).integrity,
        pkgPath: depPath,
        ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
      }
    })

  const storeIndex = new StoreIndex(storeDir)
  try {
    const modified = await pFilter(pkgs, async ({ id, integrity, depPath, name }) => {
      const pkgIndexFilePath = integrity
        ? storeIndexKey(integrity, id)
        : gitHostedStoreIndexKey(id, { built: true })
      const pkgFilesIndex = storeIndex.get(pkgIndexFilePath) as PackageFilesIndex | undefined
      if (!pkgFilesIndex) {
        return false
      }
      const { algo, files } = pkgFilesIndex
      // Transform files to dint format: { integrity: '<algo>-<base64>', size: number }
      const dintFiles: Record<string, { integrity: string, size: number }> = {}
      for (const [filePath, { digest, size }] of files) {
        dintFiles[filePath] = {
          integrity: formatIntegrity(algo, digest),
          size,
        }
      }
      return (await dint.check(path.join(virtualStoreDir, dp.depPathToFilename(depPath, maybeOpts.virtualStoreDirMaxLength), 'node_modules', name), dintFiles)) === false
    }, { concurrency: 8 })

    if ((reporter != null) && typeof reporter === 'function') {
      streamParser.removeListener('data', reporter)
    }

    return modified.map(({ pkgPath }) => pkgPath)
  } finally {
    storeIndex.close()
  }
}
